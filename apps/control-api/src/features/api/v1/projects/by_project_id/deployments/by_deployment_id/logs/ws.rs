use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use grass_node_protocol::LogStreamMessage;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        audit::{self as audits, CreateAuditEventParams},
        database::entity::AuditEventResult,
        error::AppError,
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/logs/ws",
        axum::routing::get(stream),
    )
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
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
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
    let team_id = access.team.id;
    Ok(upgrade.on_upgrade(move |socket| {
        browser_stream(state, socket, deployment_id, team_id, session, can_cancel)
    }))
}

// The request middleware records the handshake once; this separate domain
// event records completion of the stream lifecycle.
async fn record_stream_ended_audit(
    state: &ControlApiState,
    user_id: Uuid,
    team_id: Uuid,
    deployment_id: Uuid,
) {
    if let Some(db) = state.try_database() {
        audits::observe_platform_event(
            db,
            CreateAuditEventParams {
                actor_user_id: Some(user_id),
                actor_node_id: None,
                team_id: Some(team_id),
                action: "deployment.log_stream_ended".to_owned(),
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
    team_id: Uuid,
    session: Session,
    can_cancel: bool,
) {
    let user_id = session.data.user_id;
    let mut recheck = tokio::time::interval(std::time::Duration::from_secs(5));
    let mut frames = state.log_hub.subscribe(deployment_id);

    loop {
        tokio::select! {
            _ = recheck.tick() => {
                if !stream_session_valid(&state, &session).await { break; }
            }
            frame = frames.recv() => {
                match frame {
                    Ok(frame) => {
                        if !stream_session_valid(&state, &session).await { break; }
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
                        if !stream_session_valid(&state, &session).await { break; }
                        handle_ws_cancel(&state, deployment_id, user_id).await;
                    }
                    Ok(LogStreamMessage::Subscribe { .. }) => {}
                    _ => {}
                }
            }
        }
    }

    record_stream_ended_audit(&state, user_id, team_id, deployment_id).await;
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
    if let Err(error) = crate::domain::deployment_cancellation::cancel_deployment_core(
        db, cache, deployment, user_id, OP,
    )
    .await
    {
        tracing::warn!(operation = OP, %error, "websocket cancel failed");
    }
}

async fn stream_session_valid(state: &ControlApiState, session: &Session) -> bool {
    matches!(
        crate::infra::http::middlewares::session::validate_current_session(
            state,
            &session.session_id,
            "deployments.logs.ws_session"
        )
        .await,
        Ok(Some(_))
    )
}
