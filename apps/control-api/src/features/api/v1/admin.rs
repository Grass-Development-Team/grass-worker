pub mod audit_events;
pub mod host_sources;
pub mod nodes;
pub mod projects;
pub mod quota_plans;
pub mod reviews;
pub mod settings;
pub mod team_groups;
pub mod teams;
pub mod users;

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
        .route(
            "/team-groups/{group_id}",
            patch(team_groups::update).delete(team_groups::remove),
        )
        .route("/teams", get(teams::list).post(teams::create))
        .route(
            "/teams/{team_id}",
            get(teams::detail)
                .patch(teams::update)
                .delete(teams::remove),
        )
        .route("/teams/{team_id}/group", post(team_groups::assign))
        .route("/teams/{team_id}/quota-plan", post(teams::set_quota_plan))
        .route("/users", get(users::list).post(users::create))
        .route("/users/{user_id}", patch(users::update))
        .route(
            "/users/{user_id}/reset-password",
            post(users::reset_password),
        )
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/projects", get(projects::list))
        .route("/projects/{project_id}/archive", post(projects::archive))
        .route(
            "/projects/{project_id}/unarchive",
            post(projects::unarchive),
        )
        .route("/projects/{project_id}/delete", post(projects::remove))
        .route("/reviews", get(reviews::list))
        .route(
            "/deployments/{deployment_id}/review/approve",
            post(reviews::approve),
        )
        .route(
            "/deployments/{deployment_id}/review/reject",
            post(reviews::reject),
        )
        .route("/nodes", get(nodes::list).post(nodes::create))
        .route(
            "/nodes/local-process",
            get(nodes::local_process_status).post(nodes::local_process_action),
        )
        .route(
            "/nodes/{node_id}",
            get(nodes::detail).patch(nodes::update_capacity),
        )
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
