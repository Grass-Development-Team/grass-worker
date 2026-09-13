pub(crate) mod configuration;
pub(crate) mod deletion;
pub(crate) mod deletion_plan;
pub(crate) mod health;
pub(crate) mod rotate_token;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_node_protocol::NodeResources;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde::Deserialize;
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
        database::entity::{AuditEventResult, node, node_deletion_job},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/nodes/{node_id}",
            axum::routing::get(detail).patch(update_capacity),
        )
        .merge(configuration::router())
        .merge(deletion::router())
        .merge(deletion_plan::router())
        .merge(health::router())
        .merge(rotate_token::router())
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

/// GET /api/v1/admin/nodes/{node_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    Path(node_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.detail";
    let db = crate::infra::http::database(&state, OP)?;

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

    Ok(ok_response(DetailResponse {
        node: node_view(&node, usage, None, OffsetDateTime::now_utc()),
    }))
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

/// PATCH /api/v1/admin/nodes/{node_id}
pub async fn update_capacity(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Path(node_id): Path<Uuid>,
    Json(body): Json<UpdateNodeCapacityRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.update_capacity";
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

    audits::create_platform_audit_event(
        &transaction,
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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(UpdateCapacityResponse {
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
struct DetailResponse {
    node: NodeResponse,
}

#[derive(serde::Serialize)]
struct UpdateCapacityResponse {
    node: NodeResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scheduler::NodeUsage;

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
}
