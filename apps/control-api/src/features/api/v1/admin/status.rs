use axum::response::IntoResponse;

use crate::infra::error::ok_response;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/status", axum::routing::get(status))
}

async fn status() -> impl IntoResponse {
    ok_response(StatusResponse {
        service: "Grass Worker Control API",
        mode: "ready",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(serde::Serialize)]
struct StatusResponse {
    service: &'static str,
    mode: &'static str,
    version: &'static str,
}
