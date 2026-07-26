use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::{
    HeartbeatRequest, HeartbeatResponse, NodeCapabilities, RegisterRequest, RegisterResponse,
};
use serde_json::json;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        nodes::{self, RegisterNodeParams},
    },
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

/// POST /api/v1/internal/nodes/register
pub async fn register(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.register";
    let db = super::database(&state, OP)?;

    if !body.capabilities.build || !body.capabilities.serve {
        tracing::warn!(
            operation = OP,
            node = %node.name,
            build = body.capabilities.build,
            serve = body.capabilities.serve,
            "first-stage nodes must build and serve; capabilities corrected to both"
        );
    }

    let name = if body.name.trim().is_empty() {
        node.name.clone()
    } else {
        body.name.trim().to_owned()
    };

    let node = nodes::apply_registration(
        db,
        node,
        RegisterNodeParams {
            name,
            version: body.version,
            build_enabled: body.capabilities.build,
            serve_enabled: body.capabilities.serve,
            build_concurrency: i32::from(body.build_concurrency),
            base_url: body.serve_base_url,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: None,
            team_id: None,
            action: "node.registered".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "name": node.name }),
        },
    )
    .await;

    Ok(ok_response(RegisterResponse {
        node_id: node.id,
        name: node.name,
        capabilities: NodeCapabilities {
            build: true,
            serve: true,
        },
    }))
}

/// POST /api/v1/internal/nodes/heartbeat
pub async fn heartbeat(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(_body): Json<HeartbeatRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.nodes.heartbeat";
    let db = super::database(&state, OP)?;

    nodes::record_heartbeat(db, node)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(HeartbeatResponse { acknowledged: true }))
}
