use super::*;
use futures_util::{stream, stream::BoxStream};
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta,
    PutMultipartOptions, PutOptions, PutResult, UploadPart, memory::InMemory,
};
use std::sync::atomic::{AtomicBool, Ordering};
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
