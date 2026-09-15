//! Filesystem storage with atomic writes and bounded path resolution.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use super::{
    ObjectStorage, ObjectStream, OpenedArtifact, OpenedObject, StorageError, StoredArtifact,
    StoredObjectMeta,
};

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

#[cfg(test)]
#[path = "tests/local.rs"]
mod tests;
