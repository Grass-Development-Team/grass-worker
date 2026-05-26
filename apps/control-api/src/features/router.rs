use axum::{Json, Router, routing::get};
use serde::Serialize;
use tracing::info;

use crate::{features::api, features::frontend, state::ControlApiState};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

pub fn router(is_setup_mode: bool) -> Router<ControlApiState> {
    if is_setup_mode {
        info!(
            operation = "control_api.mode",
            mode = "setup",
            "Control API starting in setup mode"
        );
    } else {
        info!(
            operation = "control_api.mode",
            mode = "ready",
            "Control API starting in ready mode"
        );
    }

    Router::new()
        .route("/health", get(health))
        .nest("/api", api::router(is_setup_mode))
        .fallback(frontend::frontend_fallback)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "Grass Worker API",
        version: env!("CARGO_PKG_VERSION"),
    })
}
