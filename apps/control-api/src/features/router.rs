use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use tracing::info;

use crate::{
    features::api, features::frontend, infra::http::middlewares::session as session_mw, init,
    state::ControlApiState,
};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup: Option<bool>,
}

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    info!(operation = "control_api.start", "Control API starting");

    let session_layer =
        middleware::from_fn_with_state(state.clone(), session_mw::session_middleware);

    Router::new()
        .route("/health", get(health))
        .nest("/api", api::router(state.clone()).layer(session_layer))
        .fallback(frontend::frontend_fallback)
}

async fn health(State(state): State<ControlApiState>) -> Response {
    let in_setup_mode = match state.try_database() {
        Some(db) => match init::is_setup_finished(db).await {
            Ok(finished) => !finished,
            Err(error) => {
                tracing::error!(
                    operation = "control_api.health.database",
                    %error,
                    "health check could not read database state"
                );
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(HealthResponse {
                        status: "unavailable",
                        service: "Grass Worker API",
                        version: env!("CARGO_PKG_VERSION"),
                        setup: None,
                    }),
                )
                    .into_response();
            }
        },
        None if state.config.read().unwrap().database.url.trim().is_empty() => true,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unavailable",
                    service: "Grass Worker API",
                    version: env!("CARGO_PKG_VERSION"),
                    setup: None,
                }),
            )
                .into_response();
        }
    };

    Json(HealthResponse {
        status: "ok",
        service: "Grass Worker API",
        version: env!("CARGO_PKG_VERSION"),
        setup: in_setup_mode.then_some(true),
    })
    .into_response()
}
