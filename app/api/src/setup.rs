use crate::startup::{SetupContext, SetupStage};
use axum::{Extension, Json, Router, routing::get};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct SetupStateResponse {
    stage: SetupStage,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct SetupInfoResponse {
    service: &'static str,
    mode: &'static str,
    stage: SetupStage,
    status: &'static str,
}

pub fn install_setup(router: Router, context: SetupContext) -> Router {
    router
        .route("/api/info", get(setup_info))
        .route("/api/setup/state", get(setup_state))
        .layer(Extension(context))
}

async fn setup_info(Extension(context): Extension<SetupContext>) -> Json<SetupInfoResponse> {
    Json(SetupInfoResponse {
        service: "control-api",
        mode: "setup",
        stage: context.stage,
        status: "pending",
    })
}

async fn setup_state(Extension(context): Extension<SetupContext>) -> Json<SetupStateResponse> {
    Json(SetupStateResponse {
        stage: context.stage,
        status: "pending",
    })
}
