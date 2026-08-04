pub mod announcements;
pub mod audit_events;
pub mod build_logs;
pub mod deployments;
pub mod domains;
pub mod host_sources;
pub mod identity_providers;
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
    routing::{delete, get, patch, post},
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
            "/cleanup/audit-events",
            get(audit_events::cleanup_preview).delete(audit_events::cleanup),
        )
        .route(
            "/cleanup/build-logs",
            get(build_logs::cleanup_preview).delete(build_logs::cleanup),
        )
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
        .route(
            "/users/{user_id}/mfa",
            get(users::mfa_factors).patch(users::update_mfa_policy),
        )
        .route(
            "/users/{user_id}/mfa/{factor_id}",
            delete(users::reset_mfa_factor),
        )
        .route("/settings", get(settings::get).patch(settings::update))
        .route(
            "/identity-providers",
            get(identity_providers::list).post(identity_providers::create),
        )
        .route(
            "/identity-providers/{provider_id}",
            patch(identity_providers::update).delete(identity_providers::remove),
        )
        .route(
            "/announcements",
            get(announcements::list).post(announcements::publish),
        )
        .route(
            "/announcements/{announcement_id}",
            delete(announcements::remove),
        )
        .route("/projects", get(projects::list))
        .route("/projects/{project_id}", get(projects::detail))
        .route(
            "/projects/{project_id}/slug",
            axum::routing::patch(projects::update_slug),
        )
        .route(
            "/projects/{project_id}/deployments",
            get(projects::deployments),
        )
        .route("/projects/{project_id}/domains", get(projects::domains))
        .route("/projects/{project_id}/activity", get(projects::activity))
        .route("/projects/{project_id}/archive", post(projects::archive))
        .route(
            "/projects/{project_id}/unarchive",
            post(projects::unarchive),
        )
        .route("/projects/{project_id}/delete", post(projects::remove))
        .route(
            "/deployments/{deployment_id}/withdraw",
            post(deployments::withdraw),
        )
        .route(
            "/deployments/{deployment_id}/republish",
            post(deployments::republish),
        )
        .route("/domains/{domain_id}/approve", post(domains::approve))
        .route("/domains/{domain_id}/reject", post(domains::reject))
        .route(
            "/domains/{domain_id}",
            axum::routing::delete(domains::remove),
        )
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
        .route("/nodes/{node_id}/deletion-plan", get(nodes::deletion_plan))
        .route("/nodes/{node_id}/deletion", post(nodes::queue_deletion))
        .route(
            "/nodes/{node_id}/configuration",
            axum::routing::put(nodes::update_configuration),
        )
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

pub(crate) fn cache<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a grass_cache::CacheStore, AppError> {
    state.try_cache().ok_or_else(|| AppError::Internal {
        op,
        message: "cache not available".to_owned(),
    })
}

async fn status() -> impl IntoResponse {
    ok_response(json!({
        "service": "Grass Worker Control API",
        "mode": "ready",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
