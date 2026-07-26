pub mod audit_events;
pub mod host_sources;
pub mod nodes;
pub mod quota_plans;
pub mod team_groups;

use axum::{
    Router,
    response::IntoResponse,
    routing::{get, patch, post},
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
        .route("/audit-events", get(audit_events::list))
        .route(
            "/team-groups",
            get(team_groups::list).post(team_groups::create),
        )
        .route("/team-groups/{group_id}", patch(team_groups::update))
        .route("/teams/{team_id}/group", post(team_groups::assign))
        .route("/nodes", get(nodes::list).post(nodes::create))
        .route("/nodes/{node_id}", get(nodes::detail))
        .route("/nodes/{node_id}/health", get(nodes::health))
        .route("/nodes/{node_id}/rotate-token", post(nodes::rotate_token))
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
