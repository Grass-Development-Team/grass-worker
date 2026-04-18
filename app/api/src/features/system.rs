use crate::{SetupStage, domain::system::ApiInfo};
use axum::{Extension, Json, Router, response::IntoResponse, routing::get};
use serde::Serialize;

#[derive(Debug, Clone)]
struct SystemContext {
    info: ApiInfo,
}

#[derive(Debug, Serialize)]
struct ReadyInfoResponse {
    service: &'static str,
    mode: &'static str,
}

#[derive(Debug, Serialize)]
struct SetupInfoResponse {
    service: &'static str,
    mode: &'static str,
    stage: SetupStage,
    status: &'static str,
}

pub fn install_system_routes(router: Router, info: ApiInfo) -> Router {
    router
        .route("/api/v1/info", get(api_info))
        .layer(Extension(SystemContext { info }))
}

async fn api_info(Extension(context): Extension<SystemContext>) -> impl IntoResponse {
    match context.info {
        ApiInfo::Ready => Json(ReadyInfoResponse {
            service: "control-api",
            mode: "ready",
        })
        .into_response(),
        ApiInfo::Setup { stage } => Json(SetupInfoResponse {
            service: "control-api",
            mode: "setup",
            stage,
            status: "pending",
        })
        .into_response(),
    }
}
