use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{init, state::ControlApiState};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/health", axum::routing::get(health))
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    setup: Option<bool>,
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
