use axum::{Router, response::IntoResponse, routing::get};
use serde_json::json;

use crate::{infra::error::ok_response, state::ControlApiState};

pub fn router() -> Router<ControlApiState> {
    Router::new().route("/status", get(status))
}

async fn status() -> impl IntoResponse {
    ok_response(json!({
        "service": "Grass Worker Control API",
        "mode": "ready",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
