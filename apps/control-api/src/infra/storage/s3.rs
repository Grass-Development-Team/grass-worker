//! S3-compatible adapter and multipart upload cleanup.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use object_store::{
    ObjectStore as ApacheObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder,
    buffered::BufWriter, path::Path as ObjectPath, prefix::PrefixStore,
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::{
    ObjectStorage, ObjectStream, OpenedObject, StorageConfig, StorageCredentials, StorageError,
    StoredObjectMeta, validate_key,
};

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

#[cfg(test)]
#[path = "tests/s3.rs"]
mod tests;
