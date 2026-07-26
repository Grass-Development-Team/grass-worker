//! Build log delivery to browsers: HTTP catch-up by sequence number and the
//! websocket subscription with the cancel uplink.

use axum::{
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use grass_node_protocol::{BuildLogLine, LogStreamMessage};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments,
    },
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct BuildLogQuery {
    #[serde(default)]
    pub after_seq: Option<u64>,
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/build-log
///
/// Returns persisted log lines with `seq > after_seq` so reconnecting
/// clients can catch up before resuming the websocket stream.
pub async fn build_log(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<BuildLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.build_log";
    let access =
        crate::features::api::v1::projects::project_access(&state, &session, project_id, false, OP)
            .await?;
    let db = crate::features::api::v1::projects::database(&state, OP)?;
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .filter(|deployment| deployment.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;

    let storage = {
        let root = state.config.read().unwrap().storage.root.clone();
        crate::infra::storage::LocalStorage::new(root)
    };
    let after_seq = query.after_seq.unwrap_or(0);
    let content = storage
        .read_build_log(deployment.project_id, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .unwrap_or_default();

    let mut lines: Vec<BuildLogLine> = Vec::new();
    let mut last_seq = 0;
    for raw in content.lines() {
        let Ok(line) = serde_json::from_str::<BuildLogLine>(raw) else {
            continue;
        };
        last_seq = last_seq.max(line.seq);
        if line.seq > after_seq {
            lines.push(line);
        }
    }

    Ok(ok_response(json!({
        "lines": lines,
        "last_seq": last_seq,
        "build_status": deployments::build_status_value(&deployment.build_status),
    })))
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/logs/ws
///
/// Websocket stream of realtime frames. Downstream: log, stage_change,
/// done. Upstream: subscribe (a no-op, the path already scopes the
/// deployment) and cancel.
pub async fn stream(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
    upgrade: WebSocketUpgrade,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.logs.ws";
    let access =
        crate::features::api::v1::projects::project_access(&state, &session, project_id, false, OP)
            .await?;
    let db = crate::features::api::v1::projects::database(&state, OP)?;
    deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .filter(|deployment| deployment.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;

    let can_cancel = !matches!(
        access.role,
        crate::infra::database::entity::TeamMemberRole::Viewer
    );
    let user_id = session.data.user_id;
    Ok(upgrade.on_upgrade(move |socket| {
        browser_stream(state, socket, deployment_id, user_id, can_cancel)
    }))
}

async fn record_stream_audit(
    state: &ControlApiState,
    user_id: Uuid,
    deployment_id: Uuid,
    action: &str,
) {
    if let Some(db) = state.try_database() {
        let _ = audits::create_audit_event(
            db,
            CreateAuditEventParams {
                actor_user_id: Some(user_id),
                action: action.to_owned(),
                target_type: "deployment".to_owned(),
                target_id: Some(deployment_id),
                result: AuditEventResult::Success,
                reason: None,
                metadata: json!({}),
            },
        )
        .await;
    }
}

async fn browser_stream(
    state: ControlApiState,
    mut socket: WebSocket,
    deployment_id: Uuid,
    user_id: Uuid,
    can_cancel: bool,
) {
    record_stream_audit(
        &state,
        user_id,
        deployment_id,
        "deployment.log_stream_started",
    )
    .await;
    let mut frames = state.log_hub.subscribe(deployment_id);

    loop {
        tokio::select! {
            frame = frames.recv() => {
                match frame {
                    Ok(frame) => {
                        let Ok(text) = serde_json::to_string(&frame) else { continue };
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                        if matches!(frame, LogStreamMessage::Done { .. }) {
                            break;
                        }
                    }
                    // Lagged subscribers continue from the live position; the
                    // catch-up API fills the gap. A closed channel means the
                    // build finished before this subscriber saw Done.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { break };
                let Message::Text(text) = message else { continue };
                match serde_json::from_str::<LogStreamMessage>(&text) {
                    Ok(LogStreamMessage::Cancel { deployment_id: requested }) => {
                        if requested != deployment_id || !can_cancel {
                            continue;
                        }
                        handle_ws_cancel(&state, deployment_id, user_id).await;
                    }
                    Ok(LogStreamMessage::Subscribe { .. }) => {}
                    _ => {}
                }
            }
        }
    }

    record_stream_audit(
        &state,
        user_id,
        deployment_id,
        "deployment.log_stream_ended",
    )
    .await;
}

async fn handle_ws_cancel(state: &ControlApiState, deployment_id: Uuid, user_id: Uuid) {
    const OP: &str = "deployments.logs.ws_cancel";
    let (Some(db), Some(cache)) = (state.try_database(), state.try_cache()) else {
        return;
    };
    let deployment = match deployments::get_by_id(db, deployment_id).await {
        Ok(Some(deployment)) => deployment,
        _ => return,
    };
    if let Err(error) = crate::features::api::v1::projects::deployments::cancel_deployment_core(
        db, cache, deployment, user_id, OP,
    )
    .await
    {
        tracing::warn!(operation = OP, %error, "websocket cancel failed");
    }
}
