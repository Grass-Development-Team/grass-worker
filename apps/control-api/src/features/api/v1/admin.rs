pub mod quota_plans;

use axum::{
    Router,
    response::IntoResponse,
    routing::{get, patch},
};
use serde_json::json;

use crate::{infra::error::ok_response, state::ControlApiState};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/status", get(status))
        .route(
            "/quota-plans",
            get(quota_plans::list).post(quota_plans::create),
        )
        .route("/quota-plans/{plan_id}", patch(quota_plans::update))
}

async fn status() -> impl IntoResponse {
    ok_response(json!({
        "service": "Grass Worker Control API",
        "mode": "ready",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
