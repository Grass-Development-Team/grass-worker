use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        node_manager::config_file,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/nodes/local-process",
        axum::routing::get(local_process_status).post(local_process_action),
    )
}

/// Local managed-process block shared by the list and status endpoints.
async fn local_process_view(state: &ControlApiState) -> LocalProcessResponse {
    let (auto_start, config_path) = {
        let config = state.config.read().unwrap();
        (
            config.node_manager.auto_start_local_node,
            config.node_manager.local_node_config.clone(),
        )
    };
    LocalProcessResponse {
        auto_start,
        managed: config_file::exists(&config_path),
        process: state.node_manager.status().await,
    }
}

/// GET /api/v1/admin/nodes/local-process
pub async fn local_process_status(
    State(state): State<ControlApiState>,
) -> Result<impl IntoResponse, AppError> {
    Ok(ok_response(local_process_view(&state).await))
}

#[derive(Deserialize)]
pub struct LocalProcessActionRequest {
    pub action: LocalProcessAction,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalProcessAction {
    Start,
    Stop,
    Restart,
}

/// POST /api/v1/admin/nodes/local-process — start/stop/restart the managed
/// local node process.
pub async fn local_process_action(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Json(body): Json<LocalProcessActionRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.local_process";
    let db = crate::infra::http::database(&state, OP)?;

    let (action_name, result) = match body.action {
        LocalProcessAction::Start => ("node.local_process_started", {
            state.node_manager.start().await
        }),
        LocalProcessAction::Stop => (
            "node.local_process_stopped",
            Ok(state.node_manager.stop().await),
        ),
        LocalProcessAction::Restart => ("node.local_process_restarted", {
            state.node_manager.restart().await
        }),
    };

    match result {
        Ok(_) => {
            audits::observe_platform_event(
                db,
                CreateAuditEventParams {
                    actor_user_id: Some(data.user_id),
                    actor_node_id: None,
                    team_id: None,
                    action: action_name.to_owned(),
                    target_type: "node".to_owned(),
                    target_id: None,
                    result: AuditEventResult::Success,
                    reason: None,
                    metadata: json!({}),
                },
            )
            .await;
            Ok(ok_response(local_process_view(&state).await))
        }
        Err(error) => Err(AppError::Validation {
            op: OP,
            message: error.to_string(),
        }),
    }
}

#[derive(serde::Serialize)]
struct LocalProcessResponse {
    auto_start: bool,
    managed: bool,
    process: crate::infra::node_manager::ProcessStatus,
}
