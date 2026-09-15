//! Bounded stream reads and shared size/checksum accounting.

use std::{pin::Pin, sync::Arc};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};

use super::{OpenedObject, StorageError};

pub(super) async fn read_object_limited(
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

#[derive(Debug, Clone)]
pub(super) struct StreamStats {
    pub(super) size_bytes: u64,
    pub(super) hasher: Sha256,
}

impl Default for StreamStats {
    fn default() -> Self {
        Self {
            size_bytes: 0,
            hasher: Sha256::new(),
        }
    }
}

pub(super) struct TrackingStream<S> {
    inner: Pin<Box<S>>,
    max_bytes: u64,
    stats: Arc<std::sync::Mutex<StreamStats>>,
}

impl<S> TrackingStream<S> {
    pub(super) fn new(inner: S, max_bytes: u64, stats: Arc<std::sync::Mutex<StreamStats>>) -> Self {
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
