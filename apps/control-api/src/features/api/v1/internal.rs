pub mod deployments;
pub mod log_stream;
pub mod nodes;
pub mod serve;

use axum::{
    Router, middleware,
    routing::{get, post, put},
};

use crate::{
    infra::{error::AppError, http::middlewares::node_auth},
    state::ControlApiState,
};

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    Router::new()
        .route("/nodes/register", post(nodes::register))
        .route("/nodes/heartbeat", post(nodes::heartbeat))
        .route("/deployments/claim", post(deployments::claim))
        .route(
            "/deployments/{deployment_id}/stage",
            post(deployments::stage),
        )
        .route(
            "/deployments/{deployment_id}/build-log",
            put(deployments::append_build_log),
        )
        .route(
            "/deployments/{deployment_id}/static-site",
            put(deployments::upload_static_site),
        )
        .route(
            "/deployments/{deployment_id}/artifact",
            get(deployments::download_artifact),
        )
        .route(
            "/deployments/{deployment_id}/source-credential",
            post(deployments::redeem_source_credential),
        )
        .route(
            "/deployments/{deployment_id}/ssh-host-key",
            post(deployments::observe_ssh_host_key),
        )
        .route("/serve/assignments", get(serve::assignments))
        .route(
            "/serve/deployments/{deployment_id}/status",
            post(serve::report_status),
        )
        .route("/serve/resolve-host", get(serve::resolve_host))
        .route("/log-stream", get(log_stream::ingest))
        .layer(middleware::from_fn_with_state(
            state,
            node_auth::node_auth_middleware,
        ))
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

pub(crate) fn storage(state: &ControlApiState) -> crate::infra::storage::LocalStorage {
    let root = state.config.read().unwrap().storage.root.clone();
    crate::infra::storage::LocalStorage::new(root)
}
