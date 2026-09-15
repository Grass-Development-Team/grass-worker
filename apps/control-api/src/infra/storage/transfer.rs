//! Object enumeration and verified backend-to-backend copying.

use std::sync::Arc;

use futures_util::StreamExt;
use sha2::Digest;

use super::{
    ObjectStorage, StorageError, StoredObjectMeta,
    streams::{StreamStats, TrackingStream},
};

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
