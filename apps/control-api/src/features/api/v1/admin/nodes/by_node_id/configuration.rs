use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_node_protocol::NodeResources;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        node_deletions,
        nodes::{self, HEARTBEAT_STALE_SECONDS},
        scheduler::{self, NodeUsage},
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, NodeConfigSyncStatus, node, node_deletion_job},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/nodes/{node_id}/configuration",
        axum::routing::put(update_configuration),
    )
}

fn deletion_job_view(job: &node_deletion_job::Model) -> NodeDeletionResponse {
    NodeDeletionResponse {
        id: job.id,
        status: node_deletions::status_value(&job.status),
        target_node_id: job.target_node_id,
        total_deployments: job.total_deployments,
        migrated_deployments: job.migrated_deployments,
        active_builds: job.active_builds,
        error: job.error.clone(),
        created_at: job.created_at,
        updated_at: job.updated_at,
        completed_at: job.completed_at,
    }
}

fn node_view(
    node: &node::Model,
    usage: NodeUsage,
    deletion: Option<&node_deletion_job::Model>,
    now: OffsetDateTime,
) -> NodeResponse {
    let capacity = NodeResources {
        cpu_millicores: node.capacity_cpu_millicores.max(0) as u64,
        memory_mb: node.capacity_memory_mb.max(0) as u64,
        disk_mb: node.capacity_disk_mb.max(0) as u64,
        max_deployments: node.max_deployments.max(0) as u32,
    };
    let overflow_count = usage
        .deployments
        .saturating_sub(u64::from(capacity.max_deployments));
    NodeResponse {
        id: node.id,
        name: node.name.clone(),
        status: nodes::status_value(&node.status),
        healthy: nodes::is_healthy(node, now, HEARTBEAT_STALE_SECONDS),
        build_enabled: node.build_enabled,
        serve_enabled: node.serve_enabled,
        build_concurrency: node.build_concurrency,
        region: node.region.clone(),
        base_url: node.base_url.clone(),
        work_root: node.work_root.clone(),
        version: node.metadata.get("version").cloned(),
        capacity,
        usage,
        overflow_count,
        deletion: deletion.map(deletion_job_view),
        configuration: NodeConfigurationResponse {
            desired: node.desired_config.clone(),
            desired_revision: node.desired_config_revision,
            effective: node.effective_config.clone(),
            effective_revision: node.effective_config_revision,
            status: nodes::config_sync_status_value(&node.config_sync_status),
            error: node.config_sync_error.clone(),
            node_token_configured: node.node_token_configured,
            updated_at: node.config_updated_at,
            applied_at: node.config_applied_at,
        },
        last_heartbeat_at: node.last_heartbeat_at,
        created_at: node.created_at,
    }
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
    if grass_validator::normalize_region(&identity.region).is_err() {
        return Err("node region is invalid".to_owned());
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
    if serve.tls.enabled && (serve.tls.port == 0 || serve.tls.port == serve.port) {
        return Err("serve TLS port must be positive and different from the HTTP port".to_owned());
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
    if !matches!(
        configuration.security.gateway_authentication,
        grass_node_protocol::GatewayAuthenticationMode::Token
            | grass_node_protocol::GatewayAuthenticationMode::None
    ) {
        return Err("gateway authentication mode is invalid".to_owned());
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
async fn update_configuration(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
    Json(configuration): Json<grass_node_protocol::NodeConfiguration>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.update_configuration";
    validate_node_configuration(&configuration)
        .map_err(|message| AppError::Validation { op: OP, message })?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
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
    crate::domain::regions::require(&transaction, &configuration.node.region, OP).await?;
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

    Ok(ok_response(UpdateConfigurationResponse {
        node: node_view(&node, usage, None, OffsetDateTime::now_utc()),
    }))
}

#[derive(serde::Serialize)]
struct NodeDeletionResponse {
    id: uuid::Uuid,
    status: &'static str,
    target_node_id: Option<uuid::Uuid>,
    total_deployments: i32,
    migrated_deployments: i32,
    active_builds: i32,
    error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    completed_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct NodeConfigurationResponse {
    desired: Option<serde_json::Value>,
    desired_revision: i64,
    effective: Option<serde_json::Value>,
    effective_revision: i64,
    status: &'static str,
    error: Option<String>,
    node_token_configured: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    applied_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct NodeResponse {
    id: uuid::Uuid,
    name: String,
    status: &'static str,
    healthy: bool,
    build_enabled: bool,
    serve_enabled: bool,
    build_concurrency: i32,
    region: String,
    base_url: Option<String>,
    work_root: Option<String>,
    version: Option<serde_json::Value>,
    capacity: grass_node_protocol::NodeResources,
    usage: crate::domain::scheduler::NodeUsage,
    overflow_count: u64,
    deletion: Option<NodeDeletionResponse>,
    configuration: NodeConfigurationResponse,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_heartbeat_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UpdateConfigurationResponse {
    node: NodeResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::infra::database::entity::NodeStatus;
    use crate::infra::database::entity::node;
    use time::OffsetDateTime;
    use uuid::Uuid;
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

    #[test]
    fn node_configuration_validation_rejects_unsafe_or_unusable_values() {
        let mut configuration = configurable_node();
        assert!(validate_node_configuration(&configuration).is_ok());

        configuration.serve.tls.enabled = true;
        assert!(validate_node_configuration(&configuration).is_ok());
        configuration.serve.tls.port = 0;
        assert_eq!(
            validate_node_configuration(&configuration).unwrap_err(),
            "serve TLS port must be positive and different from the HTTP port"
        );
        configuration.serve.tls.port = configuration.serve.port;
        assert_eq!(
            validate_node_configuration(&configuration).unwrap_err(),
            "serve TLS port must be positive and different from the HTTP port"
        );
        configuration = configurable_node();

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

    fn serve_node() -> node::Model {
        let now = OffsetDateTime::now_utc();
        node::Model {
            id: Uuid::nil(),
            name: "serve-node-1".to_owned(),
            region: "default".to_owned(),
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
}
