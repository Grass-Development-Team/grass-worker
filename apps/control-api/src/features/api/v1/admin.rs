pub mod host_sources;
pub mod quota_plans;

use axum::{
    Router,
    response::IntoResponse,
    routing::{get, patch},
};
use serde_json::json;

use crate::{
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/status", get(status))
        .route(
            "/quota-plans",
            get(quota_plans::list).post(quota_plans::create),
        )
        .route("/quota-plans/{plan_id}", patch(quota_plans::update))
        .route(
            "/host-sources",
            get(host_sources::list).post(host_sources::create),
        )
        .route(
            "/host-sources/{source_id}",
            patch(host_sources::update).delete(host_sources::remove),
        )
}

pub(crate) fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}

async fn status() -> impl IntoResponse {
    ok_response(json!({
        "service": "Grass Worker Control API",
        "mode": "ready",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
