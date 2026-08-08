//! Extensible object storage for Control API artifacts.
//!
//! Business code depends on [`ObjectStorage`] and [`StorageManager`]. The
//! concrete providers are deliberately kept behind this module so adding a
//! provider does not change artifact, avatar, screenshot, or log handlers.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, StreamExt, TryStreamExt, stream::BoxStream};
use object_store::{
    ObjectStore as ApacheObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder,
    buffered::BufWriter, path::Path as ObjectPath, prefix::PrefixStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

pub type ObjectStream = BoxStream<'static, Result<Bytes, StorageError>>;

const MAX_BUILD_LOG_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("artifact exceeds the {max_bytes} byte limit")]
    LimitExceeded { max_bytes: u64 },
    #[error("artifact stream failed: {0}")]
    Stream(String),
    #[error("artifact is too large to represent")]
    UnsupportedSize,
    #[error("unsafe storage path: {0}")]
    UnsafePath(String),
    #[error("storage backend is not configured: {0}")]
    InvalidConfig(String),
    #[error("storage backend failed: {0}")]
    Backend(String),
    #[error("storage writes are temporarily disabled during migration")]
    Maintenance,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackendKind {
    #[default]
    Local,
    S3,
    Minio,
    R2,
}

impl StorageBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
            Self::Minio => "minio",
            Self::R2 => "r2",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "s3" => Some(Self::S3),
            "minio" => Some(Self::Minio),
            "r2" => Some(Self::R2),
            _ => None,
        }
    }

    pub fn default_region(self) -> &'static str {
        match self {
            Self::R2 => "auto",
            Self::Local | Self::S3 | Self::Minio => "us-east-1",
        }
    }
}

impl std::str::FromStr for StorageBackendKind {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| {
            StorageError::InvalidConfig(format!("unsupported storage backend: {value}"))
        })
    }
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: StorageBackendKind,
    #[serde(default = "default_local_root")]
    pub local_root: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(default)]
    pub allow_http: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackendKind::Local,
            local_root: default_local_root(),
            endpoint: String::new(),
            region: default_region(),
            bucket: String::new(),
            prefix: String::new(),
            force_path_style: false,
            allow_http: false,
        }
    }
}

impl StorageConfig {
    pub fn local(root: impl Into<String>) -> Self {
        Self {
            local_root: root.into(),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        let root = self.local_root.trim();
        if root.is_empty() || !Path::new(root).is_absolute() {
            return Err(StorageError::InvalidConfig(
                "local_root must be a non-empty absolute path".to_owned(),
            ));
        }
        match self.backend {
            StorageBackendKind::Local => {}
            StorageBackendKind::S3 | StorageBackendKind::Minio | StorageBackendKind::R2 => {
                if self.bucket.trim().is_empty() {
                    return Err(StorageError::InvalidConfig(
                        "bucket is required for an S3-compatible backend".to_owned(),
                    ));
                }
                if self.region.trim().is_empty() {
                    return Err(StorageError::InvalidConfig(
                        "region is required for an S3-compatible backend".to_owned(),
                    ));
                }
                if matches!(
                    self.backend,
                    StorageBackendKind::Minio | StorageBackendKind::R2
                ) && self.endpoint.trim().is_empty()
                {
                    return Err(StorageError::InvalidConfig(format!(
                        "endpoint is required for the {} backend",
                        self.backend.as_str()
                    )));
                }
                if let Some(endpoint) =
                    (!self.endpoint.trim().is_empty()).then_some(self.endpoint.trim())
                {
                    let parsed = url::Url::parse(endpoint).map_err(|error| {
                        StorageError::InvalidConfig(format!("endpoint is invalid: {error}"))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(StorageError::InvalidConfig(
                            "endpoint must use http or https".to_owned(),
                        ));
                    }
                    if parsed.scheme() == "http" && !self.allow_http {
                        return Err(StorageError::InvalidConfig(
                            "allow_http must be enabled for an http endpoint".to_owned(),
                        ));
                    }
                }
            }
        }

        if self.prefix.split('/').any(|part| part == "..") {
            return Err(StorageError::UnsafePath(self.prefix.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageCredentials {
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
}

impl StorageCredentials {
    pub fn is_configured(&self) -> bool {
        self.access_key_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .secret_access_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            || self
                .session_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StoredArtifact {
    pub relative_path: String,
    pub size_bytes: i64,
    pub checksum_sha256: String,
}

#[derive(Debug)]
pub struct OpenedArtifact {
    pub file: tokio::fs::File,
    pub size_bytes: u64,
}

pub struct OpenedObject {
    pub stream: ObjectStream,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct StoredObjectMeta {
    pub key: String,
    pub size_bytes: u64,
}

/// The provider-neutral contract used by the Control API.
#[async_trait]
pub trait ObjectStorage: Send + Sync {
    async fn put_bytes(&self, key: &str, content: &[u8]) -> Result<(), StorageError>;
    async fn put_stream(&self, key: &str, stream: ObjectStream) -> Result<(), StorageError>;
    async fn open(&self, key: &str) -> Result<Option<OpenedObject>, StorageError>;
    async fn remove(&self, key: &str) -> Result<(), StorageError>;
    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError>;
    async fn list(&self, prefix: &str) -> Result<Vec<StoredObjectMeta>, StorageError>;
    async fn probe(&self) -> Result<(), StorageError>;
}

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn artifact_relative_path(project_id: Uuid, deployment_id: Uuid) -> String {
        format!("deployments/{project_id}/{deployment_id}/grass-output.zip")
    }

    pub fn build_log_relative_path(project_id: Uuid, deployment_id: Uuid) -> String {
        format!("deployments/{project_id}/{deployment_id}/build.log")
    }

    fn resolve(&self, relative: &str) -> Result<PathBuf, StorageError> {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            return Err(StorageError::UnsafePath(relative.to_owned()));
        }
        Ok(self.root.join(relative_path))
    }

    pub async fn write_bytes(
        &self,
        relative_path: &str,
        content: &[u8],
    ) -> Result<StoredArtifact, StorageError> {
        let final_path = self.resolve(relative_path)?;
        let directory = final_path
            .parent()
            .ok_or_else(|| StorageError::UnsafePath(relative_path.to_owned()))?;
        tokio::fs::create_dir_all(directory).await?;
        let temporary_path = directory.join(format!(".write-{}.tmp", Uuid::now_v7().simple()));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await?;
        let result = async {
            file.write_all(content).await?;
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary_path, &final_path).await?;
            Ok::<(), std::io::Error>(())
        }
        .await;
        if let Err(error) = result {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary_path).await;
            return Err(error.into());
        }
        Ok(StoredArtifact {
            relative_path: relative_path.to_owned(),
            size_bytes: i64::try_from(content.len()).map_err(|_| StorageError::UnsupportedSize)?,
            checksum_sha256: hex::encode(Sha256::digest(content)),
        })
    }

    async fn put_stream_to_key(
        &self,
        key: &str,
        mut stream: ObjectStream,
    ) -> Result<(), StorageError> {
        let final_path = self.resolve(key)?;
        let directory = final_path
            .parent()
            .ok_or_else(|| StorageError::UnsafePath(key.to_owned()))?;
        tokio::fs::create_dir_all(directory).await?;
        let temporary_path = directory.join(format!(".stream-{}.tmp", Uuid::now_v7().simple()));
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await?;
        let result = async {
            while let Some(chunk) = stream.next().await {
                file.write_all(&chunk?).await?;
            }
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary_path, &final_path).await?;
            Ok::<(), StorageError>(())
        }
        .await;
        if let Err(error) = result {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary_path).await;
            return Err(error);
        }
        Ok(())
    }

    pub async fn open_artifact(
        &self,
        relative_path: &str,
    ) -> anyhow::Result<Option<OpenedArtifact>> {
        let path = self.resolve(relative_path)?;
        let file = match tokio::fs::File::open(path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata().await?;
        if !metadata.is_file() {
            anyhow::bail!("artifact path is not a regular file");
        }
        Ok(Some(OpenedArtifact {
            file,
            size_bytes: metadata.len(),
        }))
    }

    pub async fn remove(&self, relative_path: &str) -> anyhow::Result<()> {
        let path = self.resolve(relative_path)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[async_trait]
impl ObjectStorage for LocalStorage {
    async fn put_bytes(&self, key: &str, content: &[u8]) -> Result<(), StorageError> {
        self.write_bytes(key, content).await.map(|_| ())
    }

    async fn put_stream(&self, key: &str, stream: ObjectStream) -> Result<(), StorageError> {
        self.put_stream_to_key(key, stream).await
    }

    async fn open(&self, key: &str) -> Result<Option<OpenedObject>, StorageError> {
        let Some(opened) = self
            .open_artifact(key)
            .await
            .map_err(|error| StorageError::Backend(error.to_string()))?
        else {
            return Ok(None);
        };
        let stream = ReaderStream::new(opened.file)
            .map(|chunk| chunk.map_err(StorageError::Io))
            .boxed();
        Ok(Some(OpenedObject {
            stream,
            size_bytes: opened.size_bytes,
        }))
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        self.remove(key)
            .await
            .map_err(|error| StorageError::Backend(error.to_string()))
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let source = self.resolve(from)?;
        let target = self.resolve(to)?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        match tokio::fs::rename(source, target).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
                StorageError::Backend(format!("source object not found while renaming {from}")),
            ),
            Err(error) => Err(error.into()),
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<StoredObjectMeta>, StorageError> {
        let root = self.resolve(prefix)?;
        let base = self.root.clone();
        tokio::task::spawn_blocking(move || {
            if !root.exists() {
                return Ok(Vec::new());
            }
            let mut result = Vec::new();
            for entry in walkdir::WalkDir::new(root) {
                let entry = entry.map_err(|error| StorageError::Backend(error.to_string()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let key = entry
                    .path()
                    .strip_prefix(&base)
                    .map_err(|error| StorageError::Backend(error.to_string()))?
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                let size_bytes = entry
                    .metadata()
                    .map_err(|error| StorageError::Backend(error.to_string()))?
                    .len();
                result.push(StoredObjectMeta { key, size_bytes });
            }
            Ok(result)
        })
        .await
        .map_err(|error| StorageError::Backend(error.to_string()))?
    }

    async fn probe(&self) -> Result<(), StorageError> {
        let key = format!(".probe/{}", Uuid::now_v7().simple());
        self.put_bytes(&key, b"grass-storage-probe").await?;
        let result = self.open(&key).await?;
        ObjectStorage::remove(self, &key).await?;
        if result.is_none() {
            return Err(StorageError::Backend(
                "storage probe could not read its object".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct S3Storage {
    store: Arc<dyn ApacheObjectStore>,
}

impl S3Storage {
    pub fn new(
        config: &StorageConfig,
        credentials: &StorageCredentials,
    ) -> Result<Self, StorageError> {
        config.validate()?;
        let mut builder = AmazonS3Builder::from_env()
            .with_bucket_name(config.bucket.trim())
            .with_region(config.region.trim())
            .with_allow_http(config.allow_http)
            .with_virtual_hosted_style_request(!config.force_path_style);
        if !config.endpoint.trim().is_empty() {
            builder = builder.with_endpoint(config.endpoint.trim());
        }
        if let Some(value) = credentials
            .access_key_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            builder = builder.with_access_key_id(value);
        }
        if let Some(value) = credentials
            .secret_access_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            builder = builder.with_secret_access_key(value);
        }
        if let Some(value) = credentials
            .session_token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            builder = builder.with_token(value);
        }
        let store = builder
            .build()
            .map_err(|error| StorageError::Backend(error.to_string()))?;
        let store: Arc<dyn ApacheObjectStore> = if config.prefix.trim().is_empty() {
            Arc::new(store)
        } else {
            Arc::new(PrefixStore::new(
                store,
                ObjectPath::from(config.prefix.trim()),
            ))
        };
        Ok(Self { store })
    }

    fn path(&self, key: &str) -> Result<ObjectPath, StorageError> {
        validate_key(key)?;
        ObjectPath::parse(key).map_err(|error| StorageError::UnsafePath(error.to_string()))
    }
}

#[async_trait]
impl ObjectStorage for S3Storage {
    async fn put_bytes(&self, key: &str, content: &[u8]) -> Result<(), StorageError> {
        self.store
            .put(
                &self.path(key)?,
                PutPayload::from(Bytes::copy_from_slice(content)),
            )
            .await
            .map(|_| ())
            .map_err(|error| StorageError::Backend(error.to_string()))
    }

    async fn put_stream(&self, key: &str, mut stream: ObjectStream) -> Result<(), StorageError> {
        let mut writer =
            BufWriter::with_capacity(Arc::clone(&self.store), self.path(key)?, 10 * 1024 * 1024);
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let _ = writer.abort().await;
                    return Err(error);
                }
            };
            match writer.put(chunk).await {
                Ok(()) => {}
                Err(error) => {
                    let _ = writer.abort().await;
                    return Err(StorageError::Backend(error.to_string()));
                }
            }
        }
        writer
            .shutdown()
            .await
            .map_err(|error| StorageError::Backend(error.to_string()))
    }

    async fn open(&self, key: &str) -> Result<Option<OpenedObject>, StorageError> {
        let path = self.path(key)?;
        let result = match self.store.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(StorageError::Backend(error.to_string())),
        };
        let size_bytes = result.meta.size;
        let stream = result
            .into_stream()
            .map(|chunk| chunk.map_err(|error| StorageError::Backend(error.to_string())))
            .boxed();
        Ok(Some(OpenedObject { stream, size_bytes }))
    }

    async fn remove(&self, key: &str) -> Result<(), StorageError> {
        match self.store.delete(&self.path(key)?).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(StorageError::Backend(error.to_string())),
        }
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.store
            .rename(&self.path(from)?, &self.path(to)?)
            .await
            .map_err(|error| StorageError::Backend(error.to_string()))
    }

    async fn list(&self, prefix: &str) -> Result<Vec<StoredObjectMeta>, StorageError> {
        let prefix = if prefix.trim().is_empty() {
            None
        } else {
            Some(self.path(prefix)?)
        };
        self.store
            .list(prefix.as_ref())
            .map_ok(|meta| StoredObjectMeta {
                key: meta.location.to_string(),
                size_bytes: meta.size,
            })
            .try_collect()
            .await
            .map_err(|error| StorageError::Backend(error.to_string()))
    }

    async fn probe(&self) -> Result<(), StorageError> {
        let key = format!(".probe/{}", Uuid::now_v7().simple());
        self.put_bytes(&key, b"grass-storage-probe").await?;
        let result = self.open(&key).await?;
        self.remove(&key).await?;
        if result.is_none() {
            return Err(StorageError::Backend(
                "storage probe could not read its object".to_owned(),
            ));
        }
        Ok(())
    }
}

fn default_local_root() -> String {
    "/data".to_owned()
}

fn default_region() -> String {
    "us-east-1".to_owned()
}

fn validate_key(key: &str) -> Result<(), StorageError> {
    let path = Path::new(key);
    if key.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(StorageError::UnsafePath(key.to_owned()));
    }
    Ok(())
}

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
    ) -> Result<PendingArtifact, StorageError>
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

async fn read_object_limited(
    object: OpenedObject,
    max_bytes: u64,
) -> Result<Vec<u8>, StorageError> {
    if object.size_bytes > max_bytes {
        return Err(StorageError::LimitExceeded { max_bytes });
    }
    let capacity = usize::try_from(object.size_bytes).map_err(|_| StorageError::UnsupportedSize)?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut size_bytes = 0_u64;
    let mut stream = object.stream;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        size_bytes = size_bytes
            .checked_add(u64::try_from(chunk.len()).map_err(|_| StorageError::UnsupportedSize)?)
            .ok_or(StorageError::UnsupportedSize)?;
        if size_bytes > max_bytes {
            return Err(StorageError::LimitExceeded { max_bytes });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub fn build_backend(
    config: &StorageConfig,
    credentials: &StorageCredentials,
) -> Result<Arc<dyn ObjectStorage>, StorageError> {
    config.validate()?;
    match config.backend {
        StorageBackendKind::Local => Ok(Arc::new(LocalStorage::new(config.local_root.clone()))),
        StorageBackendKind::S3 | StorageBackendKind::Minio | StorageBackendKind::R2 => {
            Ok(Arc::new(S3Storage::new(config, credentials)?))
        }
    }
}

pub async fn list_managed_backend(
    backend: &Arc<dyn ObjectStorage>,
) -> Result<Vec<StoredObjectMeta>, StorageError> {
    let mut objects = backend.list("deployments/").await?;
    objects.extend(backend.list("avatars/").await?);
    objects.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(objects)
}

pub async fn copy_and_verify(
    source: &Arc<dyn ObjectStorage>,
    target: &Arc<dyn ObjectStorage>,
    key: &str,
) -> Result<(u64, String), StorageError> {
    let source_object = source
        .open(key)
        .await?
        .ok_or_else(|| StorageError::Backend(format!("source object disappeared: {key}")))?;
    let source_size = source_object.size_bytes;
    let source_stats = Arc::new(std::sync::Mutex::new(StreamStats::default()));
    let tracked = TrackingStream::new(source_object.stream, u64::MAX, Arc::clone(&source_stats));
    target.put_stream(key, Box::pin(tracked)).await?;
    let source_stats = source_stats.lock().unwrap().clone();
    if source_stats.size_bytes != source_size {
        return Err(StorageError::Backend(format!(
            "source object size changed while copying {key}"
        )));
    }
    let source_checksum = hex::encode(source_stats.hasher.finalize());

    let target_object = target.open(key).await?.ok_or_else(|| {
        StorageError::Backend(format!("target object is missing after copy: {key}"))
    })?;
    let target_size = target_object.size_bytes;
    let target_stats = Arc::new(std::sync::Mutex::new(StreamStats::default()));
    let mut tracked =
        TrackingStream::new(target_object.stream, u64::MAX, Arc::clone(&target_stats));
    while let Some(chunk) = tracked.next().await {
        chunk?;
    }
    let target_stats = target_stats.lock().unwrap().clone();
    let target_checksum = hex::encode(target_stats.hasher.finalize());
    if target_size != source_size
        || target_stats.size_bytes != source_size
        || target_checksum != source_checksum
    {
        return Err(StorageError::Backend(format!(
            "target verification failed for {key}"
        )));
    }
    Ok((source_size, source_checksum))
}

#[derive(Debug, Clone)]
struct StreamStats {
    size_bytes: u64,
    hasher: Sha256,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            size_bytes: 0,
            hasher: Sha256::new(),
        }
    }
}

struct TrackingStream<S> {
    inner: Pin<Box<S>>,
    max_bytes: u64,
    stats: Arc<std::sync::Mutex<StreamStats>>,
}

impl<S> TrackingStream<S> {
    fn new(inner: S, max_bytes: u64, stats: Arc<std::sync::Mutex<StreamStats>>) -> Self {
        Self {
            inner: Box::pin(inner),
            max_bytes,
            stats,
        }
    }
}

impl<S, E> Stream for TrackingStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<Bytes, StorageError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(chunk))) => {
                let mut stats = self.stats.lock().unwrap();
                let next_size = stats.size_bytes.saturating_add(chunk.len() as u64);
                if next_size > self.max_bytes {
                    return std::task::Poll::Ready(Some(Err(StorageError::LimitExceeded {
                        max_bytes: self.max_bytes,
                    })));
                }
                stats.size_bytes = next_size;
                stats.hasher.update(&chunk);
                std::task::Poll::Ready(Some(Ok(chunk)))
            }
            std::task::Poll::Ready(Some(Err(error))) => {
                std::task::Poll::Ready(Some(Err(StorageError::Stream(error.to_string()))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
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
mod tests {
    use bytes::Bytes;
    use futures_util::stream;
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta,
        PutMultipartOptions, PutOptions, PutPayload, PutResult, UploadPart, memory::InMemory,
    };

    use super::*;

    #[derive(Debug)]
    struct AbortTrackingStore {
        inner: InMemory,
        aborted: Arc<AtomicBool>,
    }

    impl std::fmt::Display for AbortTrackingStore {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("abort-tracking-store")
        }
    }

    #[async_trait]
    impl ApacheObjectStore for AbortTrackingStore {
        async fn put_opts(
            &self,
            location: &ObjectPath,
            payload: PutPayload,
            options: PutOptions,
        ) -> object_store::Result<PutResult> {
            self.inner.put_opts(location, payload, options).await
        }

        async fn put_multipart_opts(
            &self,
            location: &ObjectPath,
            options: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            let inner = self.inner.put_multipart_opts(location, options).await?;
            Ok(Box::new(AbortTrackingUpload {
                inner,
                aborted: Arc::clone(&self.aborted),
            }))
        }

        async fn get_opts(
            &self,
            location: &ObjectPath,
            options: GetOptions,
        ) -> object_store::Result<GetResult> {
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<ObjectPath>>,
        ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&ObjectPath>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &ObjectPath,
            to: &ObjectPath,
            options: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    #[derive(Debug)]
    struct AbortTrackingUpload {
        inner: Box<dyn MultipartUpload>,
        aborted: Arc<AtomicBool>,
    }

    #[async_trait]
    impl MultipartUpload for AbortTrackingUpload {
        fn put_part(&mut self, data: PutPayload) -> UploadPart {
            self.inner.put_part(data)
        }

        async fn complete(&mut self) -> object_store::Result<PutResult> {
            self.inner.complete().await
        }

        async fn abort(&mut self) -> object_store::Result<()> {
            self.aborted.store(true, Ordering::Release);
            self.inner.abort().await
        }
    }

    #[test]
    fn storage_config_defaults_to_local_and_rejects_unsafe_paths() {
        let config = StorageConfig::default();
        assert_eq!(config.backend, StorageBackendKind::Local);
        assert!(config.validate().is_ok());
        assert!(validate_key("../outside").is_err());
        assert!(validate_key("/absolute").is_err());
    }

    #[test]
    fn minio_and_r2_require_explicit_endpoints() {
        for backend in [StorageBackendKind::Minio, StorageBackendKind::R2] {
            let config = StorageConfig {
                backend,
                bucket: "artifacts".to_owned(),
                region: "us-east-1".to_owned(),
                ..StorageConfig::default()
            };
            assert!(
                config.validate().is_err(),
                "{backend:?} accepted a missing endpoint"
            );
        }
    }

    #[test]
    fn remote_storage_requires_an_absolute_local_node_root() {
        let config = StorageConfig {
            backend: StorageBackendKind::S3,
            local_root: "relative-node-root".to_owned(),
            endpoint: "https://s3.example.com".to_owned(),
            bucket: "artifacts".to_owned(),
            region: "us-east-1".to_owned(),
            ..StorageConfig::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn provider_default_regions_preserve_backend_semantics() {
        assert_eq!(StorageBackendKind::R2.default_region(), "auto");
        assert_eq!(StorageBackendKind::S3.default_region(), "us-east-1");
        assert_eq!(StorageBackendKind::Minio.default_region(), "us-east-1");
    }

    #[tokio::test]
    async fn s3_stream_error_aborts_multipart_upload() {
        let aborted = Arc::new(AtomicBool::new(false));
        let store: Arc<dyn ApacheObjectStore> = Arc::new(AbortTrackingStore {
            inner: InMemory::new(),
            aborted: Arc::clone(&aborted),
        });
        let storage = S3Storage { store };
        let chunks = stream::iter([
            Ok(Bytes::from(vec![0; 10 * 1024 * 1024])),
            Err(StorageError::Stream("injected failure".to_owned())),
        ])
        .boxed();

        let error = storage
            .put_stream("multipart.bin", chunks)
            .await
            .unwrap_err();

        assert!(matches!(error, StorageError::Stream(_)));
        assert!(aborted.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn build_log_append_rejects_content_over_the_hard_limit() {
        let dir = std::env::temp_dir().join(format!("grass-storage-log-limit-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let content = "x".repeat(16 * 1024 * 1024 + 1);

        let error = manager
            .append_build_log(Uuid::now_v7(), Uuid::now_v7(), &content)
            .await
            .unwrap_err();

        assert!(matches!(error, StorageError::LimitExceeded { .. }));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn build_log_read_rejects_an_oversized_stored_object() {
        let dir =
            std::env::temp_dir().join(format!("grass-storage-log-read-limit-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let key = LocalStorage::build_log_relative_path(project_id, deployment_id);
        manager
            .write_bytes(&key, &vec![b'x'; 16 * 1024 * 1024 + 1])
            .await
            .unwrap();

        let error = manager
            .read_build_log(project_id, deployment_id)
            .await
            .unwrap_err();

        assert!(matches!(error, StorageError::LimitExceeded { .. }));
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[derive(Clone)]
    struct BlockingLogStorage {
        blocked_key: String,
        blocked_started: Arc<tokio::sync::Notify>,
        release_blocked: Arc<tokio::sync::Notify>,
        other_started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl ObjectStorage for BlockingLogStorage {
        async fn put_bytes(&self, _key: &str, _content: &[u8]) -> Result<(), StorageError> {
            Ok(())
        }

        async fn put_stream(
            &self,
            _key: &str,
            mut stream: ObjectStream,
        ) -> Result<(), StorageError> {
            while stream.next().await.transpose()?.is_some() {}
            Ok(())
        }

        async fn open(&self, key: &str) -> Result<Option<OpenedObject>, StorageError> {
            if key == self.blocked_key {
                self.blocked_started.notify_one();
                self.release_blocked.notified().await;
            } else {
                self.other_started.notify_one();
            }
            Ok(None)
        }

        async fn remove(&self, _key: &str) -> Result<(), StorageError> {
            Ok(())
        }

        async fn rename(&self, _from: &str, _to: &str) -> Result<(), StorageError> {
            Ok(())
        }

        async fn list(&self, _prefix: &str) -> Result<Vec<StoredObjectMeta>, StorageError> {
            Ok(Vec::new())
        }

        async fn probe(&self) -> Result<(), StorageError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn build_log_appends_for_independent_deployments_do_not_share_a_lock() {
        let project_id = Uuid::now_v7();
        let blocked_deployment = Uuid::now_v7();
        let other_deployment = Uuid::now_v7();
        let backend = Arc::new(BlockingLogStorage {
            blocked_key: LocalStorage::build_log_relative_path(project_id, blocked_deployment),
            blocked_started: Arc::new(tokio::sync::Notify::new()),
            release_blocked: Arc::new(tokio::sync::Notify::new()),
            other_started: Arc::new(tokio::sync::Notify::new()),
        });
        let blocked_started = Arc::clone(&backend.blocked_started);
        let release_blocked = Arc::clone(&backend.release_blocked);
        let other_started = Arc::clone(&backend.other_started);
        let manager = StorageManager::from_runtime(StorageRuntime {
            config: StorageConfig::local("/tmp/grass-storage-test"),
            backend,
        });

        let blocked = tokio::spawn({
            let manager = manager.clone();
            async move {
                manager
                    .append_build_log(project_id, blocked_deployment, "blocked")
                    .await
            }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            blocked_started.notified(),
        )
        .await
        .expect("blocked deployment did not reach storage");

        let other = tokio::spawn({
            let manager = manager.clone();
            async move {
                manager
                    .append_build_log(project_id, other_deployment, "other")
                    .await
            }
        });
        let independent = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            other_started.notified(),
        )
        .await
        .is_ok();

        release_blocked.notify_one();
        blocked.await.unwrap().unwrap();
        other.await.unwrap().unwrap();
        assert!(
            independent,
            "independent deployment remained behind another log lock"
        );
    }

    #[tokio::test]
    async fn local_storage_round_trip_and_list() {
        let dir = std::env::temp_dir().join(format!("grass-storage-test-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let key = format!("avatars/users/{}/avatar.webp", Uuid::now_v7());
        let stored = manager.write_bytes(&key, b"webp-bytes").await.unwrap();
        assert_eq!(stored.size_bytes, 10);
        let opened = manager.open_artifact(&key).await.unwrap().unwrap();
        let bytes = opened
            .stream
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(bytes, b"webp-bytes");
        assert_eq!(
            list_managed_backend(&manager.backend())
                .await
                .unwrap()
                .len(),
            1
        );
        manager.remove(&key).await.unwrap();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn local_rename_rejects_a_missing_source_object() {
        let dir = std::env::temp_dir().join(format!("grass-storage-rename-{}", Uuid::now_v7()));
        let storage = LocalStorage::new(&dir);

        let error = ObjectStorage::rename(
            &storage,
            ".pending/missing",
            "deployments/project/deployment/grass-output.zip",
        )
        .await
        .unwrap_err();

        assert!(matches!(error, StorageError::Backend(_)));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn maintenance_rejects_writes_but_keeps_reads_available() {
        let dir =
            std::env::temp_dir().join(format!("grass-storage-maintenance-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let key = format!("avatars/users/{}/avatar.webp", Uuid::now_v7());
        manager.write_bytes(&key, b"existing").await.unwrap();

        manager.mark_maintenance();

        assert!(manager.open_artifact(&key).await.unwrap().is_some());
        assert!(matches!(
            manager.write_bytes("avatars/new.webp", b"new").await,
            Err(StorageError::Maintenance)
        ));
        assert!(matches!(
            manager
                .append_build_log(Uuid::now_v7(), Uuid::now_v7(), "log")
                .await,
            Err(StorageError::Maintenance)
        ));
        assert!(matches!(
            manager.remove(&key).await,
            Err(StorageError::Maintenance)
        ));

        manager.leave_maintenance();
        manager.remove(&key).await.unwrap();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn streamed_artifact_is_hashed_and_limited() {
        let dir = std::env::temp_dir().join(format!("grass-storage-stream-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let chunks = stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static(b"zip-")),
            Ok(Bytes::from_static(b"bytes")),
        ]);
        let pending = manager
            .write_artifact_stream(Uuid::now_v7(), Uuid::now_v7(), chunks, 9)
            .await
            .unwrap();
        assert_eq!(pending.size_bytes, 9);
        let stored = pending.finalize().await.unwrap();
        assert_eq!(stored.size_bytes, 9);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn dropped_pending_artifact_removes_temporary_object() {
        let dir = std::env::temp_dir().join(format!("grass-storage-drop-{}", Uuid::now_v7()));
        let manager = StorageManager::new_local(&dir);
        let pending = manager
            .write_artifact_stream(
                Uuid::now_v7(),
                Uuid::now_v7(),
                stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"pending"))]),
                64,
            )
            .await
            .unwrap();
        let temporary_path = dir.join(pending.temporary_key.as_deref().unwrap());
        assert!(tokio::fs::try_exists(&temporary_path).await.unwrap());

        drop(pending);

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while tokio::fs::try_exists(&temporary_path).await.unwrap() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("pending object cleanup timed out");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
