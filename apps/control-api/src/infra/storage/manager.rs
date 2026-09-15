//! Backend switching, maintenance leases and pending artifact writes.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use bytes::Bytes;
use futures_util::Stream;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    LocalStorage, MAX_BUILD_LOG_BYTES, ObjectStorage, OpenedObject, StorageConfig,
    StorageCredentials, StorageError, StoredArtifact, build_backend,
    streams::{StreamStats, TrackingStream, read_object_limited},
    validate_key,
};

#[derive(Clone)]
struct StorageRuntime {
    config: StorageConfig,
    backend: Arc<dyn ObjectStorage>,
}

#[derive(Clone)]
pub struct StorageManager {
    runtime: Arc<RwLock<StorageRuntime>>,
    write_gate: Arc<tokio::sync::RwLock<()>>,
    maintenance: Arc<AtomicBool>,
    log_append_locks: Arc<std::sync::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>,
}

pub(crate) struct StorageWriteGuard {
    _guard: tokio::sync::OwnedRwLockReadGuard<()>,
    backend: Arc<dyn ObjectStorage>,
}

impl StorageWriteGuard {
    pub(crate) async fn write_bytes(
        &self,
        key: &str,
        content: &[u8],
    ) -> Result<StoredArtifact, StorageError> {
        validate_key(key)?;
        self.backend.put_bytes(key, content).await?;
        Ok(StoredArtifact {
            relative_path: key.to_owned(),
            size_bytes: i64::try_from(content.len()).map_err(|_| StorageError::UnsupportedSize)?,
            checksum_sha256: hex::encode(Sha256::digest(content)),
        })
    }

    pub(crate) async fn remove(&self, key: &str) -> Result<(), StorageError> {
        validate_key(key)?;
        self.backend.remove(key).await
    }
}

impl StorageManager {
    pub fn build_log_relative_path(project_id: Uuid, deployment_id: Uuid) -> String {
        LocalStorage::build_log_relative_path(project_id, deployment_id)
    }

    pub fn new_local(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let backend: Arc<dyn ObjectStorage> = Arc::new(LocalStorage::new(root.clone()));
        Self::from_runtime(StorageRuntime {
            config: StorageConfig::local(root.to_string_lossy()),
            backend,
        })
    }

    fn from_runtime(runtime: StorageRuntime) -> Self {
        Self {
            runtime: Arc::new(RwLock::new(runtime)),
            write_gate: Arc::new(tokio::sync::RwLock::new(())),
            maintenance: Arc::new(AtomicBool::new(false)),
            log_append_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn config(&self) -> StorageConfig {
        self.runtime.read().unwrap().config.clone()
    }

    pub fn backend(&self) -> Arc<dyn ObjectStorage> {
        Arc::clone(&self.runtime.read().unwrap().backend)
    }

    pub fn replace(
        &self,
        config: StorageConfig,
        credentials: StorageCredentials,
    ) -> Result<(), StorageError> {
        let backend = build_backend(&config, &credentials)?;
        self.replace_backend(config, backend);
        Ok(())
    }

    pub fn replace_backend(&self, config: StorageConfig, backend: Arc<dyn ObjectStorage>) {
        let mut runtime = self.runtime.write().unwrap();
        runtime.config = config;
        runtime.backend = backend;
    }

    pub fn is_maintenance(&self) -> bool {
        self.maintenance.load(Ordering::Acquire)
    }

    pub fn mark_maintenance(&self) {
        self.maintenance.store(true, Ordering::Release);
    }

    pub async fn enter_maintenance(&self) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        self.mark_maintenance();
        self.write_gate.clone().write_owned().await
    }

    pub fn leave_maintenance(&self) {
        self.maintenance.store(false, Ordering::Release);
    }

    async fn write_lock(&self) -> Result<tokio::sync::OwnedRwLockReadGuard<()>, StorageError> {
        if self.is_maintenance() {
            return Err(StorageError::Maintenance);
        }
        let guard = self.write_gate.clone().read_owned().await;
        if self.is_maintenance() {
            drop(guard);
            return Err(StorageError::Maintenance);
        }
        Ok(guard)
    }

    pub(crate) async fn begin_write(&self) -> Result<StorageWriteGuard, StorageError> {
        let guard = self.write_lock().await?;
        Ok(StorageWriteGuard {
            _guard: guard,
            backend: self.backend(),
        })
    }

    pub async fn write_bytes(
        &self,
        key: &str,
        content: &[u8],
    ) -> Result<StoredArtifact, StorageError> {
        self.begin_write().await?.write_bytes(key, content).await
    }

    pub async fn write_artifact_stream<S, E>(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
        stream: S,
        max_bytes: u64,
    ) -> Result<super::PendingArtifact, StorageError>
    where
        S: Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        let write_guard = self.write_lock().await?;
        let final_key = LocalStorage::artifact_relative_path(project_id, deployment_id);
        let temporary_key = format!(".pending/{}", Uuid::now_v7().simple());
        let stats = Arc::new(std::sync::Mutex::new(StreamStats::default()));
        let tracked = TrackingStream::new(stream, max_bytes, Arc::clone(&stats));
        let backend = self.backend();
        if let Err(error) = backend.put_stream(&temporary_key, Box::pin(tracked)).await {
            let _ = backend.remove(&temporary_key).await;
            return Err(error);
        }
        let stats = stats.lock().unwrap().clone();
        Ok(PendingArtifact {
            storage: backend,
            temporary_key: Some(temporary_key),
            final_key,
            size_bytes: i64::try_from(stats.size_bytes)
                .map_err(|_| StorageError::UnsupportedSize)?,
            checksum_sha256: hex::encode(stats.hasher.finalize()),
            write_guard: Some(write_guard),
        })
    }

    pub async fn open_artifact(&self, key: &str) -> Result<Option<OpenedObject>, StorageError> {
        validate_key(key)?;
        self.backend().open(key).await
    }

    pub async fn remove(&self, key: &str) -> Result<(), StorageError> {
        self.begin_write().await?.remove(key).await
    }

    pub async fn append_build_log(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
        content: &str,
    ) -> Result<(), StorageError> {
        let lock_key = format!("{project_id}:{deployment_id}");
        let append_lock = self.log_append_lock(&lock_key);
        let _append_guard = append_lock.lock().await;
        let _write_guard = self.write_lock().await?;
        let content_size =
            u64::try_from(content.len()).map_err(|_| StorageError::UnsupportedSize)?;
        if content_size > MAX_BUILD_LOG_BYTES {
            return Err(StorageError::LimitExceeded {
                max_bytes: MAX_BUILD_LOG_BYTES,
            });
        }
        let key = LocalStorage::build_log_relative_path(project_id, deployment_id);
        let backend = self.backend();
        let mut current = match backend.open(&key).await? {
            Some(object) => {
                if object.size_bytes > MAX_BUILD_LOG_BYTES.saturating_sub(content_size) {
                    return Err(StorageError::LimitExceeded {
                        max_bytes: MAX_BUILD_LOG_BYTES,
                    });
                }
                read_object_limited(object, MAX_BUILD_LOG_BYTES - content_size).await?
            }
            None => Vec::new(),
        };
        if u64::try_from(current.len())
            .ok()
            .and_then(|size| size.checked_add(content_size))
            .is_none_or(|size| size > MAX_BUILD_LOG_BYTES)
        {
            return Err(StorageError::LimitExceeded {
                max_bytes: MAX_BUILD_LOG_BYTES,
            });
        }
        current.extend_from_slice(content.as_bytes());
        backend.put_bytes(&key, &current).await
    }

    pub async fn read_build_log(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<Option<String>, StorageError> {
        let key = LocalStorage::build_log_relative_path(project_id, deployment_id);
        let Some(object) = self.backend().open(&key).await? else {
            return Ok(None);
        };
        let bytes = read_object_limited(object, MAX_BUILD_LOG_BYTES).await?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| StorageError::Backend(format!("build log is not UTF-8: {error}")))
    }

    fn log_append_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.log_append_locks.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

pub struct PendingArtifact {
    storage: Arc<dyn ObjectStorage>,
    temporary_key: Option<String>,
    final_key: String,
    pub size_bytes: i64,
    pub checksum_sha256: String,
    write_guard: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
}

impl PendingArtifact {
    pub async fn finalize(mut self) -> Result<StoredArtifact, StorageError> {
        let temporary_key = self
            .temporary_key
            .take()
            .expect("pending artifact must own a temporary object");
        if let Err(error) = self.storage.rename(&temporary_key, &self.final_key).await {
            let _ = self.storage.remove(&temporary_key).await;
            return Err(error);
        }
        self.write_guard.take();
        Ok(StoredArtifact {
            relative_path: self.final_key.clone(),
            size_bytes: self.size_bytes,
            checksum_sha256: self.checksum_sha256.clone(),
        })
    }

    pub async fn discard(mut self) {
        if let Some(temporary_key) = self.temporary_key.take() {
            let _ = self.storage.remove(&temporary_key).await;
        }
        self.write_guard.take();
    }
}

impl Drop for PendingArtifact {
    fn drop(&mut self) {
        let Some(temporary_key) = self.temporary_key.take() else {
            return;
        };
        let storage = Arc::clone(&self.storage);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = storage.remove(&temporary_key).await;
            });
        }
    }
}

#[cfg(test)]
#[path = "tests/manager.rs"]
mod tests;
