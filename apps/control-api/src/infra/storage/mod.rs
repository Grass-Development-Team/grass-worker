//! Extensible object storage for Control API artifacts.
//!
//! Business code depends on [`ObjectStorage`] and [`StorageManager`]. The
//! concrete providers are deliberately kept behind this module so adding a
//! provider does not change artifact, avatar, screenshot, or log handlers.

mod config;
mod local;
mod manager;
mod s3;
mod streams;
mod transfer;

pub use config::{StorageBackendKind, StorageConfig, StorageCredentials};
pub use local::LocalStorage;
pub(crate) use manager::StorageWriteGuard;
pub use manager::{PendingArtifact, StorageManager};
pub use s3::S3Storage;
pub use transfer::{copy_and_verify, list_managed_backend};

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::BoxStream;

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
