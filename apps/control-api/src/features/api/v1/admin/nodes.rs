use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        nodes::{self, CreateNodeParams},
    },
    infra::{
        database::entity::{AuditEventResult, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::revoked_token_key,
    },
    state::ControlApiState,
};

/// Heartbeats older than this mark a Node unhealthy.
pub const HEARTBEAT_STALE_SECONDS: i64 = 90;

fn node_view(node: &node::Model, now: OffsetDateTime) -> serde_json::Value {
    json!({
        "id": node.id,
        "name": node.name,
        "status": nodes::status_value(&node.status),
        "healthy": nodes::is_healthy(node, now, HEARTBEAT_STALE_SECONDS),
        "build_enabled": node.build_enabled,
        "serve_enabled": node.serve_enabled,
        "build_concurrency": node.build_concurrency,
        "base_url": node.base_url,
        "work_root": node.work_root,
        "version": node.metadata.get("version"),
        "last_heartbeat_at": node.last_heartbeat_at,
        "created_at": node.created_at,
    })
}

/// GET /api/v1/admin/nodes
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.list";
    let db = super::database(&state, OP)?;

    // Lazily flip stale Active nodes to Offline so the list reflects
    // reality even between background sweeps.
    let _ = nodes::mark_stale_offline(db, HEARTBEAT_STALE_SECONDS).await;

    let nodes = nodes::list(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let now = OffsetDateTime::now_utc();

    Ok(ok_response(json!({
        "nodes": nodes.iter().map(|node| node_view(node, now)).collect::<Vec<_>>(),
    })))
}

/// GET /api/v1/admin/nodes/{node_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.detail";
    let db = super::database(&state, OP)?;

    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;

    Ok(ok_response(json!({
        "node": node_view(&node, OffsetDateTime::now_utc()),
    })))
}

/// GET /api/v1/admin/nodes/{node_id}/health
pub async fn health(
    State(state): State<ControlApiState>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.health";
    let db = super::database(&state, OP)?;

    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;

    let now = OffsetDateTime::now_utc();
    Ok(ok_response(json!({
        "node_id": node.id,
        "status": nodes::status_value(&node.status),
        "healthy": nodes::is_healthy(&node, now, HEARTBEAT_STALE_SECONDS),
        "last_heartbeat_at": node.last_heartbeat_at,
        "seconds_since_heartbeat": node
            .last_heartbeat_at
            .map(|at| (now - at).whole_seconds()),
    })))
}

#[derive(Deserialize)]
pub struct CreateNodeRequest {
    pub name: String,
}

/// POST /api/v1/admin/nodes — creates a Node and returns its token once.
pub async fn create(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Json(body): Json<CreateNodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.create";
    let db = super::database(&state, OP)?;

    if body.name.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "name is required".to_owned(),
        });
    }

    let token = grass_token::generate_token();
    let node = nodes::create_node(
        db,
        CreateNodeParams {
            name: body.name.trim().to_owned(),
            token_hash: grass_token::hash_token(&token),
            storage_root: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: None,
            action: "node.created".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "name": node.name }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "node": node_view(&node, OffsetDateTime::now_utc()),
        // Shown exactly once; only the hash is stored.
        "token": token,
    })))
}

/// POST /api/v1/admin/nodes/{node_id}/rotate-token
///
/// Revokes the current token immediately and returns a new one once.
pub async fn rotate_token(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.rotate_token";
    let db = super::database(&state, OP)?;

    let node = nodes::get_by_id(db, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;

    let old_hash = node.token_hash.clone();
    let token = grass_token::generate_token();
    let node = nodes::replace_token_hash(db, node, grass_token::hash_token(&token))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    // Blacklist the old token so revocation applies before any cache of the
    // node row expires.
    if let Some(cache) = state.try_cache() {
        use grass_cache::Cache;
        let _ = cache
            .set(
                &revoked_token_key(&old_hash),
                "1",
                std::time::Duration::from_secs(60 * 60 * 24 * 30),
            )
            .await;
    }

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            team_id: None,
            action: "node.token_revoked".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: Some("token rotated".to_owned()),
            metadata: json!({}),
        },
    )
    .await;

    Ok(ok_response(json!({
        "node_id": node.id,
        "token": token,
    })))
}
