use std::future::Future;

use uuid::Uuid;

tokio::task_local! {
    static REQUEST_ID: Uuid;
}

pub(super) async fn scope<T>(request_id: Uuid, future: impl Future<Output = T>) -> T {
    REQUEST_ID.scope(request_id, future).await
}

pub(super) fn request_id() -> Option<Uuid> {
    REQUEST_ID.try_with(|id| *id).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditErrorContext {
    pub operation: &'static str,
    pub reason: String,
}
