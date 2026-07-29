use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_node_protocol::NodeResources;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, TransactionTrait};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        nodes::{self, CreateNodeParams},
        scheduler::{self, NodeUsage},
        settings,
    },
    infra::{
        database::entity::{AuditEventResult, NodeConfigSyncStatus, node},
        error::{AppError, ok_response},
        http::middlewares::node_auth::revoked_token_key,
        node_manager::config_file,
    },
    state::ControlApiState,
};

/// Heartbeats older than this mark a Node unhealthy.
pub const HEARTBEAT_STALE_SECONDS: i64 = 90;

fn node_view(node: &node::Model, usage: NodeUsage, now: OffsetDateTime) -> serde_json::Value {
    let capacity = NodeResources {
        cpu_millicores: node.capacity_cpu_millicores.max(0) as u64,
        memory_mb: node.capacity_memory_mb.max(0) as u64,
        disk_mb: node.capacity_disk_mb.max(0) as u64,
        max_deployments: node.max_deployments.max(0) as u32,
    };
    let overflow_count = usage
        .deployments
        .saturating_sub(u64::from(capacity.max_deployments));
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
        "capacity": capacity,
        "usage": usage,
        "overflow_count": overflow_count,
        "configuration": {
            "desired": node.desired_config,
            "desired_revision": node.desired_config_revision,
            "effective": node.effective_config,
            "effective_revision": node.effective_config_revision,
            "status": nodes::config_sync_status_value(&node.config_sync_status),
            "error": node.config_sync_error,
            "node_token_configured": node.node_token_configured,
            "updated_at": ts(node.config_updated_at),
            "applied_at": ts(node.config_applied_at),
        },
        "last_heartbeat_at": ts(node.last_heartbeat_at),
        "created_at": ts(node.created_at),
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
    let usage = scheduler::node_usage(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let now = OffsetDateTime::now_utc();

    Ok(ok_response(json!({
        "nodes": nodes
            .iter()
            .map(|node| node_view(node, usage.get(&node.id).copied().unwrap_or_default(), now))
            .collect::<Vec<_>>(),
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
    let usage = scheduler::node_usage(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .remove(&node_id)
        .unwrap_or_default();

    Ok(ok_response(json!({
        "node": node_view(&node, usage, OffsetDateTime::now_utc()),
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateNodeCapacityRequest {
    pub capacity_cpu_millicores: u64,
    pub capacity_memory_mb: u64,
    pub capacity_disk_mb: u64,
    pub max_deployments: u32,
}

fn validate_capacity(
    body: &UpdateNodeCapacityRequest,
    usage: NodeUsage,
) -> Result<NodeResources, String> {
    if body.capacity_cpu_millicores == 0
        || body.capacity_memory_mb == 0
        || body.capacity_disk_mb == 0
        || body.max_deployments == 0
    {
        return Err("capacity values must be positive integers".to_owned());
    }
    if body.capacity_cpu_millicores > i64::MAX as u64
        || body.capacity_memory_mb > i64::MAX as u64
        || body.capacity_disk_mb > i64::MAX as u64
        || body.max_deployments > i32::MAX as u32
    {
        return Err("capacity values exceed the supported range".to_owned());
    }
    if body.capacity_cpu_millicores < usage.cpu_millicores {
        return Err(format!(
            "CPU capacity cannot be lower than current usage ({}m)",
            usage.cpu_millicores
        ));
    }
    if body.capacity_memory_mb < usage.memory_mb {
        return Err(format!(
            "memory capacity cannot be lower than current usage ({} MB)",
            usage.memory_mb
        ));
    }
    if body.capacity_disk_mb < usage.disk_mb {
        return Err(format!(
            "disk capacity cannot be lower than current usage ({} MB)",
            usage.disk_mb
        ));
    }
    if u64::from(body.max_deployments) < usage.deployments {
        return Err(format!(
            "deployment capacity cannot be lower than current usage ({})",
            usage.deployments
        ));
    }
    Ok(NodeResources {
        cpu_millicores: body.capacity_cpu_millicores,
        memory_mb: body.capacity_memory_mb,
        disk_mb: body.capacity_disk_mb,
        max_deployments: body.max_deployments,
    })
}

fn validate_http_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https") && url.has_host() && url.username().is_empty()
    })
}

fn validate_node_configuration(
    configuration: &grass_node_protocol::NodeConfiguration,
) -> Result<(), String> {
    let identity = &configuration.node;
    if identity.id.trim().is_empty() || identity.id.chars().count() > 120 {
        return Err("node id must contain between 1 and 120 characters".to_owned());
    }
    if !validate_http_url(identity.control_api.trim()) {
        return Err("control API must be an absolute HTTP(S) URL without credentials".to_owned());
    }
    if !std::path::Path::new(identity.work_root.trim()).is_absolute() {
        return Err("node work root must be an absolute path".to_owned());
    }
    if !identity.capabilities.build && !identity.capabilities.serve {
        return Err("node must enable build or serve".to_owned());
    }
    if identity.capabilities.build && configuration.build.concurrency == 0 {
        return Err("build concurrency must be positive for a Build Node".to_owned());
    }
    if configuration.build.command_timeout_seconds == 0 {
        return Err("build command timeout must be greater than zero".to_owned());
    }

    let serve = &configuration.serve;
    if serve.host.parse::<std::net::IpAddr>().is_err() {
        return Err("serve host must be an IPv4 or IPv6 address".to_owned());
    }
    if serve.port == 0 {
        return Err("serve port must be greater than zero".to_owned());
    }
    if !validate_http_url(serve.public_base_url.trim()) {
        return Err("serve public base URL must be an absolute HTTP(S) URL".to_owned());
    }
    if !std::path::Path::new(serve.artifact_cache_root.trim()).is_absolute() {
        return Err("artifact cache root must be an absolute path".to_owned());
    }
    if serve.capacity.max_deployments == 0 {
        return Err("maximum deployments must be greater than zero".to_owned());
    }
    if serve.capacity.cpu_millicores > i64::MAX as u64
        || serve.capacity.memory_mb > i64::MAX as u64
        || serve.capacity.disk_mb > i64::MAX as u64
        || serve.capacity.max_deployments > i32::MAX as u32
    {
        return Err("serve capacity exceeds the supported range".to_owned());
    }

    let runtime = &configuration.runtime;
    if !matches!(runtime.backend.as_str(), "docker-socket" | "podman-socket") {
        return Err("runtime backend must be docker-socket or podman-socket".to_owned());
    }
    if runtime.socket.trim().is_empty() {
        return Err("runtime socket cannot be empty".to_owned());
    }
    if runtime.default_build_image.trim().is_empty()
        || runtime.default_serve_image.trim().is_empty()
    {
        return Err("runtime images cannot be empty".to_owned());
    }
    if runtime.network.trim().is_empty() {
        return Err("runtime network cannot be empty".to_owned());
    }
    if runtime.resources.cpu_limit == 0 || runtime.resources.memory_mb == 0 {
        return Err("runtime resource limits must be greater than zero".to_owned());
    }
    if runtime.resources.memory_mb > i64::MAX as u64 {
        return Err("runtime memory limit exceeds the supported range".to_owned());
    }

    if configuration.security.private_repository_targets.len() > 100 {
        return Err("no more than 100 private repository targets may be configured".to_owned());
    }
    for target in &configuration.security.private_repository_targets {
        let host = target.host.trim();
        if host.is_empty() || host.contains('*') || host.contains('/') {
            return Err("private repository targets require an exact host".to_owned());
        }
        if target.ip.parse::<std::net::IpAddr>().is_err() {
            return Err("private repository target IP is invalid".to_owned());
        }
        if target.port == 0 {
            return Err("private repository target port must be greater than zero".to_owned());
        }
    }
    if tracing_subscriber::EnvFilter::try_new(configuration.log.level.trim()).is_err() {
        return Err("log filter is invalid".to_owned());
    }
    Ok(())
}

fn validate_configuration_capacity(
    configuration: &grass_node_protocol::NodeConfiguration,
    usage: NodeUsage,
) -> Result<(), String> {
    if !configuration.node.capabilities.serve {
        return Ok(());
    }
    let capacity = configuration.serve.capacity;
    if capacity.cpu_millicores != 0 && capacity.cpu_millicores < usage.cpu_millicores {
        return Err(format!(
            "CPU capacity cannot be lower than current usage ({}m)",
            usage.cpu_millicores
        ));
    }
    if capacity.memory_mb != 0 && capacity.memory_mb < usage.memory_mb {
        return Err(format!(
            "memory capacity cannot be lower than current usage ({} MB)",
            usage.memory_mb
        ));
    }
    if capacity.disk_mb != 0 && capacity.disk_mb < usage.disk_mb {
        return Err(format!(
            "disk capacity cannot be lower than current usage ({} MB)",
            usage.disk_mb
        ));
    }
    if u64::from(capacity.max_deployments) < usage.deployments {
        return Err(format!(
            "deployment capacity cannot be lower than current usage ({})",
            usage.deployments
        ));
    }
    Ok(())
}

struct PreparedDesiredConfigurationUpdate {
    desired: serde_json::Value,
    revision: i64,
    status: NodeConfigSyncStatus,
    error: Option<String>,
    updated_at: OffsetDateTime,
}

fn prepare_desired_configuration_update(
    node: &node::Model,
    configuration: &grass_node_protocol::NodeConfiguration,
    now: OffsetDateTime,
) -> Result<PreparedDesiredConfigurationUpdate, String> {
    validate_node_configuration(configuration)?;
    let revision = node
        .desired_config_revision
        .checked_add(1)
        .ok_or_else(|| "node configuration revision is exhausted".to_owned())?;
    let desired = serde_json::to_value(configuration)
        .map_err(|error| format!("node configuration cannot be serialized: {error}"))?;
    Ok(PreparedDesiredConfigurationUpdate {
        desired,
        revision,
        status: NodeConfigSyncStatus::Pending,
        error: None,
        updated_at: now,
    })
}

/// PUT /api/v1/admin/nodes/{node_id}/configuration
pub async fn update_configuration(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
    Json(configuration): Json<grass_node_protocol::NodeConfiguration>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.update_configuration";
    validate_node_configuration(&configuration)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    scheduler::lock_placement(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let node = nodes::get_by_id_for_update(&transaction, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;
    let usage = scheduler::node_usage(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .remove(&node_id)
        .unwrap_or_default();
    validate_configuration_capacity(&configuration, usage)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let prepared =
        prepare_desired_configuration_update(&node, &configuration, OffsetDateTime::now_utc())
            .map_err(|message| AppError::Validation { op: OP, message })?;
    let before = json!({
        "configuration": node.desired_config,
        "revision": node.desired_config_revision,
    });
    let after = json!({
        "configuration": prepared.desired,
        "revision": prepared.revision,
    });
    let mut active: node::ActiveModel = node.into();
    active.desired_config = Set(Some(prepared.desired));
    active.desired_config_revision = Set(prepared.revision);
    active.config_sync_status = Set(prepared.status);
    active.config_sync_error = Set(prepared.error);
    active.config_updated_at = Set(Some(prepared.updated_at));
    let node = active
        .update(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    audits::create_platform_audit_event_with_changes(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "node.configuration_updated".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "revision": node.desired_config_revision }),
        },
        json!({ "before": before, "after": after }),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({
        "node": node_view(&node, usage, OffsetDateTime::now_utc()),
    })))
}

/// PATCH /api/v1/admin/nodes/{node_id}
pub async fn update_capacity(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
    Json(body): Json<UpdateNodeCapacityRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.update_capacity";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    scheduler::lock_placement(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let node = nodes::get_by_id(&transaction, node_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "node not found".to_owned(),
        })?;
    if !node.serve_enabled {
        return Err(AppError::Validation {
            op: OP,
            message: "capacity can only be configured for a Serve Node".to_owned(),
        });
    }
    let usage = scheduler::node_usage(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .remove(&node_id)
        .unwrap_or_default();
    let capacity = validate_capacity(&body, usage)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let old = json!({
        "capacity_cpu_millicores": node.capacity_cpu_millicores,
        "capacity_memory_mb": node.capacity_memory_mb,
        "capacity_disk_mb": node.capacity_disk_mb,
        "max_deployments": node.max_deployments,
    });
    let new = json!({
        "capacity_cpu_millicores": capacity.cpu_millicores,
        "capacity_memory_mb": capacity.memory_mb,
        "capacity_disk_mb": capacity.disk_mb,
        "max_deployments": capacity.max_deployments,
    });
    let mut active: node::ActiveModel = node.into();
    active.capacity_cpu_millicores = Set(capacity.cpu_millicores as i64);
    active.capacity_memory_mb = Set(capacity.memory_mb as i64);
    active.capacity_disk_mb = Set(capacity.disk_mb as i64);
    active.max_deployments = Set(capacity.max_deployments as i32);
    let node = active
        .update(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "node.capacity_updated".to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(node.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "old": old, "new": new }),
        },
    )
    .await;

    Ok(ok_response(json!({
        "node": node_view(&node, usage, OffsetDateTime::now_utc()),
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
        "last_heartbeat_at": ts(node.last_heartbeat_at),
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

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
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
                        let _ = audits::create_platform_audit_event(
                            db,
                            CreateAuditEventParams {
                                actor_user_id: Some(data.user_id),
                                actor_node_id: None,
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
        "node": node_view(&node, NodeUsage::default(), OffsetDateTime::now_utc()),
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
            let _ = audits::create_platform_audit_event(
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

    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{domain::scheduler::NodeUsage, infra::database::entity::NodeStatus};

    fn configurable_node() -> grass_node_protocol::NodeConfiguration {
        serde_json::from_value(json!({
            "node": {
                "id": "node-a",
                "control_api": "https://control.example.test",
                "work_root": "/data/node",
                "capabilities": { "build": true, "serve": true }
            },
            "build": {
                "concurrency": 2,
                "command_timeout_seconds": 600,
                "retain_workspace_on_failure": false
            },
            "serve": {
                "host": "0.0.0.0",
                "port": 8080,
                "public_base_url": "https://node-a.example.test",
                "metadata_cache_ttl_seconds": 30,
                "artifact_cache_root": "/data/node/artifacts",
                "capacity": {
                    "cpu_millicores": 2_000,
                    "memory_mb": 4_096,
                    "disk_mb": 20_480,
                    "max_deployments": 20
                },
                "ssr": { "idle_stop_seconds": 1_800, "startup_timeout_seconds": 90 }
            },
            "runtime": {
                "backend": "podman-socket",
                "socket": "unix:///run/user/1000/podman/podman.sock",
                "default_build_image": "docker.io/library/node:22",
                "default_serve_image": "docker.io/library/node:22",
                "network": "bridge",
                "resources": { "cpu_limit": 2, "memory_mb": 2_048 }
            },
            "security": {
                "private_repository_targets": [
                    { "host": "git.internal.example", "ip": "10.0.0.8", "port": 2222 }
                ]
            },
            "development": { "verbose_build_log": false },
            "log": { "level": "info", "format": "pretty" }
        }))
        .unwrap()
    }

    fn serve_node() -> node::Model {
        let now = OffsetDateTime::now_utc();
        node::Model {
            id: Uuid::nil(),
            name: "serve-node-1".to_owned(),
            token_hash: String::new(),
            status: NodeStatus::Active,
            build_enabled: false,
            serve_enabled: true,
            build_concurrency: 0,
            base_url: Some("http://node-1:8080".to_owned()),
            work_root: None,
            capacity_cpu_millicores: 1_200,
            capacity_memory_mb: 1_536,
            capacity_disk_mb: 8_192,
            max_deployments: 10,
            metadata: json!({ "version": "0.1.0" }),
            last_heartbeat_at: Some(now),
            desired_config: None,
            desired_config_revision: 0,
            effective_config: None,
            effective_config_revision: 0,
            config_sync_status: crate::infra::database::entity::NodeConfigSyncStatus::Pending,
            config_sync_error: None,
            node_token_configured: false,
            config_updated_at: None,
            config_applied_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn request() -> UpdateNodeCapacityRequest {
        UpdateNodeCapacityRequest {
            capacity_cpu_millicores: 1_600,
            capacity_memory_mb: 2_048,
            capacity_disk_mb: 16_384,
            max_deployments: 20,
        }
    }

    #[test]
    fn capacity_request_requires_positive_values() {
        let mut body = request();
        body.capacity_cpu_millicores = 0;

        let error = validate_capacity(&body, NodeUsage::default()).unwrap_err();

        assert_eq!(error, "capacity values must be positive integers");
    }

    #[test]
    fn capacity_request_cannot_drop_below_current_usage() {
        let body = request();
        let usage = NodeUsage {
            cpu_millicores: 1_601,
            memory_mb: 256,
            disk_mb: 512,
            deployments: 1,
        };

        let error = validate_capacity(&body, usage).unwrap_err();

        assert_eq!(
            error,
            "CPU capacity cannot be lower than current usage (1601m)"
        );
    }

    #[test]
    fn node_configuration_validation_rejects_unsafe_or_unusable_values() {
        let mut configuration = configurable_node();
        assert!(validate_node_configuration(&configuration).is_ok());

        configuration.node.capabilities.build = false;
        configuration.node.capabilities.serve = false;
        assert_eq!(
            validate_node_configuration(&configuration).unwrap_err(),
            "node must enable build or serve"
        );

        configuration = configurable_node();
        configuration.node.work_root = "relative/work".to_owned();
        assert_eq!(
            validate_node_configuration(&configuration).unwrap_err(),
            "node work root must be an absolute path"
        );

        configuration = configurable_node();
        configuration.runtime.backend = "unknown".to_owned();
        assert_eq!(
            validate_node_configuration(&configuration).unwrap_err(),
            "runtime backend must be docker-socket or podman-socket"
        );
    }

    #[test]
    fn desired_configuration_update_increments_revision_and_resets_sync_state() {
        let mut node = serve_node();
        node.desired_config = Some(serde_json::to_value(configurable_node()).unwrap());
        node.desired_config_revision = 6;
        node.effective_config_revision = 5;
        node.config_sync_status = crate::infra::database::entity::NodeConfigSyncStatus::Failed;
        node.config_sync_error = Some("old failure".to_owned());
        let desired = configurable_node();

        let update =
            prepare_desired_configuration_update(&node, &desired, OffsetDateTime::UNIX_EPOCH)
                .unwrap();

        assert_eq!(update.revision, 7);
        assert_eq!(update.desired, serde_json::to_value(desired).unwrap());
        assert_eq!(
            update.status,
            crate::infra::database::entity::NodeConfigSyncStatus::Pending
        );
        assert!(update.error.is_none());
        assert_eq!(update.updated_at, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(node.effective_config_revision, 5);
    }

    #[test]
    fn node_view_reports_capacity_usage_and_overflow() {
        let node = serve_node();
        let usage = NodeUsage {
            cpu_millicores: 1_400,
            memory_mb: 512,
            disk_mb: 1_024,
            deployments: 12,
        };

        let view = node_view(&node, usage, OffsetDateTime::now_utc());

        assert_eq!(view["capacity"]["cpu_millicores"], 1_200);
        assert_eq!(view["usage"]["deployments"], 12);
        assert_eq!(view["overflow_count"], 2);
        assert_eq!(view["configuration"]["status"], "pending");
        assert_eq!(view["configuration"]["desired_revision"], 0);
        assert_eq!(view["configuration"]["effective_revision"], 0);
        assert!(view["configuration"]["desired"].is_null());
        assert!(view["configuration"]["effective"].is_null());
        assert_eq!(view["configuration"]["node_token_configured"], false);
    }
}
