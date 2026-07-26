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

    /// Writes the grass-output archive for a deployment and returns its
    /// checksum and size.
    pub async fn save_artifact(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
        bytes: &[u8],
    ) -> anyhow::Result<StoredArtifact> {
        let dir = self.deployment_dir(project_id, deployment_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("grass-output.zip");
        tokio::fs::write(&path, bytes).await?;

        let checksum = hex::encode(Sha256::digest(bytes));
        Ok(StoredArtifact {
            relative_path: Self::artifact_relative_path(project_id, deployment_id),
            size_bytes: bytes.len() as i64,
            checksum_sha256: checksum,
        })
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

    pub async fn read_artifact(
        &self,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let path = self
            .deployment_dir(project_id, deployment_id)
            .join("grass-output.zip");
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
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
    async fn artifacts_round_trip_with_checksums() {
        let dir = std::env::temp_dir().join(format!("grass-storage-test-{}", Uuid::now_v7()));
        let storage = LocalStorage::new(&dir);
        let project_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();

        let stored = storage
            .save_artifact(project_id, deployment_id, b"zip-bytes")
            .await
            .unwrap();
        assert_eq!(stored.size_bytes, 9);
        assert_eq!(
            stored.checksum_sha256,
            hex::encode(Sha256::digest(b"zip-bytes"))
        );

        let read = storage
            .read_artifact(project_id, deployment_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read, b"zip-bytes");

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
}
