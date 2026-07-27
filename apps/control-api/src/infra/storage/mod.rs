//! Local filesystem storage for deployment artifacts and build logs.
//!
//! Layout under the configured root:
//!
//! ```text
//! <root>/deployments/<project_id>/<deployment_id>/build.log
//! <root>/deployments/<project_id>/<deployment_id>/grass-output.zip
//! ```
//!
//! Path segments are UUIDs rendered by us, so no user-controlled path parts
//! ever reach the filesystem.

use std::path::{Path, PathBuf};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

pub struct StoredArtifact {
    pub relative_path: String,
    pub size_bytes: i64,
    pub checksum_sha256: String,
}

pub struct OpenedArtifact {
    pub file: tokio::fs::File,
    pub size_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("artifact exceeds the {max_bytes} byte limit")]
    LimitExceeded { max_bytes: u64 },
    #[error("artifact stream failed: {0}")]
    Stream(String),
    #[error("artifact is too large to represent")]
    UnsupportedSize,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct PendingArtifact {
    temporary_path: Option<PathBuf>,
    final_path: PathBuf,
    pub relative_path: String,
    pub size_bytes: i64,
    pub checksum_sha256: String,
}

impl PendingArtifact {
    #[cfg(test)]
    pub fn temporary_path(&self) -> &Path {
        self.temporary_path
            .as_deref()
            .expect("pending artifact has already been consumed")
    }

    pub async fn finalize(mut self) -> Result<StoredArtifact, StorageError> {
        let temporary_path = self
            .temporary_path
            .take()
            .expect("pending artifact has already been consumed");
        if let Err(error) = tokio::fs::rename(&temporary_path, &self.final_path).await {
            self.temporary_path = Some(temporary_path);
            return Err(error.into());
        }
        Ok(StoredArtifact {
            relative_path: self.relative_path.clone(),
            size_bytes: self.size_bytes,
            checksum_sha256: self.checksum_sha256.clone(),
        })
    }

    pub async fn discard(mut self) {
        if let Some(path) = self.temporary_path.take() {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

impl Drop for PendingArtifact {
    fn drop(&mut self) {
        if let Some(path) = self.temporary_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[allow(dead_code)] // Read by serve-side helpers in Milestone 10.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn deployment_dir(&self, project_id: Uuid, deployment_id: Uuid) -> PathBuf {
        self.root
            .join("deployments")
            .join(project_id.to_string())
            .join(deployment_id.to_string())
    }

    pub fn artifact_relative_path(project_id: Uuid, deployment_id: Uuid) -> String {
        format!("deployments/{project_id}/{deployment_id}/grass-output.zip")
    }

    pub fn build_log_relative_path(project_id: Uuid, deployment_id: Uuid) -> String {
        format!("deployments/{project_id}/{deployment_id}/build.log")
    }

    /// Resolves a stored relative path, refusing anything that escapes the
    /// storage root.
    #[allow(dead_code)] // Wired by artifact readers in Milestone 9.
    pub fn resolve(&self, relative: &str) -> anyhow::Result<PathBuf> {
        let relative_path = Path::new(relative);
        if relative_path.is_absolute()
            || relative_path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
        {
            anyhow::bail!("unsafe storage path: {relative}");
        }
        Ok(self.root.join(relative_path))
    }

    pub async fn write_artifact_stream<S, E>(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
        stream: S,
        max_bytes: u64,
    ) -> Result<PendingArtifact, StorageError>
    where
        S: Stream<Item = Result<Bytes, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        let dir = self.deployment_dir(project_id, deployment_id);
        tokio::fs::create_dir_all(&dir).await?;
        let temporary_path = dir.join(format!(".upload-{}.tmp", Uuid::now_v7().simple()));
        let final_path = dir.join("grass-output.zip");
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await?;
        let mut hasher = Sha256::new();
        let mut size_bytes = 0_u64;
        futures_util::pin_mut!(stream);

        let result = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| StorageError::Stream(error.to_string()))?;
                let next_size = size_bytes
                    .checked_add(chunk.len() as u64)
                    .ok_or(StorageError::UnsupportedSize)?;
                if next_size > max_bytes {
                    return Err(StorageError::LimitExceeded { max_bytes });
                }
                file.write_all(&chunk).await?;
                hasher.update(&chunk);
                size_bytes = next_size;
            }
            file.flush().await?;
            file.sync_all().await?;
            Ok(())
        }
        .await;

        if let Err(error) = result {
            drop(file);
            let _ = tokio::fs::remove_file(&temporary_path).await;
            return Err(error);
        }

        Ok(PendingArtifact {
            temporary_path: Some(temporary_path),
            final_path,
            relative_path: Self::artifact_relative_path(project_id, deployment_id),
            size_bytes: i64::try_from(size_bytes).map_err(|_| StorageError::UnsupportedSize)?,
            checksum_sha256: hex::encode(hasher.finalize()),
        })
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

    /// Appends log lines to the deployment build log.
    pub async fn append_build_log(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
        content: &str,
    ) -> anyhow::Result<()> {
        let dir = self.deployment_dir(project_id, deployment_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("build.log");
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(content.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    pub async fn read_build_log(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> anyhow::Result<Option<String>> {
        let path = self
            .deployment_dir(project_id, deployment_id)
            .join("build.log");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use futures_util::stream;

    use super::*;

    #[test]
    fn resolve_rejects_traversal_and_absolute_paths() {
        let storage = LocalStorage::new("/tmp/grass-test");
        assert!(storage.resolve("deployments/a/b/grass-output.zip").is_ok());
        assert!(storage.resolve("../etc/passwd").is_err());
        assert!(storage.resolve("deployments/../../etc/passwd").is_err());
        assert!(storage.resolve("/etc/passwd").is_err());
    }

    #[tokio::test]
    async fn build_logs_round_trip() {
        let dir = std::env::temp_dir().join(format!("grass-storage-test-{}", Uuid::now_v7()));
        let storage = LocalStorage::new(&dir);
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();

        storage
            .append_build_log(project_id, deployment_id, "line 1\n")
            .await
            .unwrap();
        storage
            .append_build_log(project_id, deployment_id, "line 2\n")
            .await
            .unwrap();
        let log = storage
            .read_build_log(project_id, deployment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(log, "line 1\nline 2\n");

        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn artifact_stream_is_hashed_limited_and_atomically_finalized() {
        let dir = std::env::temp_dir().join(format!("grass-storage-stream-{}", Uuid::now_v7()));
        let storage = LocalStorage::new(&dir);
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let chunks = stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static(b"zip-")),
            Ok(Bytes::from_static(b"bytes")),
        ]);

        let pending = storage
            .write_artifact_stream(project_id, deployment_id, chunks, 9)
            .await
            .unwrap();
        assert_eq!(pending.size_bytes, 9);
        assert_eq!(
            pending.checksum_sha256,
            hex::encode(Sha256::digest(b"zip-bytes"))
        );
        let temporary_path = pending.temporary_path().to_owned();
        assert!(temporary_path.is_file());
        assert!(!storage.resolve(&pending.relative_path).unwrap().exists());

        let stored = pending.finalize().await.unwrap();

        assert!(!temporary_path.exists());
        assert!(storage.resolve(&stored.relative_path).unwrap().is_file());
        let opened = storage
            .open_artifact(&stored.relative_path)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(opened.size_bytes, 9);
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }

    #[tokio::test]
    async fn oversized_artifact_stream_removes_the_temporary_file() {
        let dir = std::env::temp_dir().join(format!("grass-storage-limit-{}", Uuid::now_v7()));
        let storage = LocalStorage::new(&dir);
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let chunks = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"too-large"))]);

        let error = storage
            .write_artifact_stream(project_id, deployment_id, chunks, 4)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("exceeds"));
        let deployment_dir = storage.deployment_dir(project_id, deployment_id);
        let mut entries = tokio::fs::read_dir(deployment_dir).await.unwrap();
        assert!(entries.next_entry().await.unwrap().is_none());
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
