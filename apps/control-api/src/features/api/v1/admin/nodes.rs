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
        settings,
    },
    infra::{
        database::entity::{AuditEventResult, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::revoked_token_key,
        node_manager::config_file,
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
        "local_process": local_process_view(&state).await,
    })))
}

/// Local managed-process block shared by the list and status endpoints.
async fn local_process_view(state: &ControlApiState) -> serde_json::Value {
    let (auto_start, config_path) = {
        let config = state.config.read().unwrap();
        (
            config.node_manager.auto_start_local_node,
            config.node_manager.local_node_config.clone(),
        )
    };
    json!({
        "auto_start": auto_start,
        "managed": config_file::exists(&config_path),
        "process": state.node_manager.status().await,
    })
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
    /// Generate the local node config and start the managed process.
    #[serde(default)]
    pub start_local: bool,
}

/// POST /api/v1/admin/nodes — creates a Node and returns its token once.
/// With `start_local`, the managed node config is generated from that token
/// and the local process is started immediately.
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
            metadata: json!({ "name": node.name, "start_local": body.start_local }),
        },
    )
    .await;

    let mut warnings = Vec::new();
    let mut local_process = None;
    if body.start_local {
        let storage_root = settings::get_setting(db, "storage.root")
            .await
            .ok()
            .flatten()
            .and_then(|setting| setting.value.as_str().map(str::to_owned))
            .unwrap_or_else(|| state.config.read().unwrap().storage.root.clone());
        let (config_path, control_api_url) = {
            let config = state.config.read().unwrap();
            (
                config.node_manager.local_node_config.clone(),
                config_file::control_api_url(config.server.host, config.server.port),
            )
        };

        match config_file::generate(
            &config_path,
            &config_file::GenerateParams {
                node_name: &node.name,
                node_token: &token,
                control_api_url,
                storage_root: &storage_root,
            },
        ) {
            Ok(mut generated_warnings) => {
                warnings.append(&mut generated_warnings);
                match state.node_manager.start().await {
                    Ok(status) => {
                        local_process = Some(status);
                        let _ = audits::create_audit_event(
                            db,
                            CreateAuditEventParams {
                                actor_user_id: Some(data.user_id),
                                team_id: None,
                                action: "node.local_process_started".to_owned(),
                                target_type: "node".to_owned(),
                                target_id: Some(node.id),
                                result: AuditEventResult::Success,
                                reason: None,
                                metadata: json!({}),
                            },
                        )
                        .await;
                    }
                    Err(error) => {
                        warnings.push(format!("failed to start local node process: {error}"));
                    }
                }
            }
            Err(error) => warnings.push(format!("failed to write local node config: {error}")),
        }
    }

    Ok(ok_response(json!({
        "node": node_view(&node, OffsetDateTime::now_utc()),
        // Shown exactly once; only the hash is stored.
        "token": token,
        "local_process": local_process,
        "warnings": warnings,
    })))
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
    let db = super::database(&state, OP)?;

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
            let _ = audits::create_audit_event(
                db,
                CreateAuditEventParams {
                    actor_user_id: Some(data.user_id),
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
