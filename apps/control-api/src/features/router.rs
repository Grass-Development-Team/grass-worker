use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use tracing::info;

use crate::{features::api, features::frontend, init, state::ControlApiState};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup: Option<bool>,
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

async fn health(State(state): State<ControlApiState>) -> Json<HealthResponse> {
    let in_setup_mode = match state.try_database() {
        Some(db) => !init::is_setup_finished(db).await.unwrap_or(true),
        None => true,
    };

    Json(HealthResponse {
        status: "ok",
        service: "Grass Worker API",
        version: env!("CARGO_PKG_VERSION"),
        setup: in_setup_mode.then_some(true),
    })
}
