use axum::{Json, Router, extract::State, middleware, routing::get};
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
