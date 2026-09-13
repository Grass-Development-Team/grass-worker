pub(crate) mod by_node_id;
pub(crate) mod local_process;

use axum::{Json, extract::State, response::IntoResponse};
use grass_node_protocol::NodeResources;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use crate::infra::audit as audits;
use crate::{
    domain::{
        node_deletions,
        nodes::{self, CreateNodeParams, HEARTBEAT_STALE_SECONDS},
        scheduler::{self, NodeUsage},
        settings,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, node, node_deletion_job},
        error::{AppError, ok_response},
        node_manager::config_file,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/nodes", axum::routing::get(list).post(create))
        .merge(by_node_id::router())
        .merge(local_process::router())
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

/// GET /api/v1/admin/nodes
async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.list";
    let db = crate::infra::http::database(&state, OP)?;

    // Lazily flip stale Active nodes to Offline so the list reflects
    // reality even between background sweeps.
    let _ = nodes::mark_stale_offline(db, HEARTBEAT_STALE_SECONDS).await;

    let nodes = nodes::list(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let deletion_jobs = node_deletion_job::Entity::find()
        .filter(
            node_deletion_job::Column::Status
                .ne(crate::infra::database::entity::NodeDeletionStatus::Completed),
        )
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .into_iter()
        .map(|job| (job.node_id, job))
        .collect::<std::collections::HashMap<_, _>>();
    let usage = scheduler::node_usage(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let now = OffsetDateTime::now_utc();

    Ok(ok_response(ListResponse {
        nodes: nodes
            .iter()
            .map(|node| {
                node_view(
                    node,
                    usage.get(&node.id).copied().unwrap_or_default(),
                    deletion_jobs.get(&node.id),
                    now,
                )
            })
            .collect::<Vec<_>>(),
        local_process: local_process_view(&state).await,
    }))
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

#[derive(Deserialize)]
struct CreateNodeRequest {
    name: String,
    #[serde(default)]
    region: Option<String>,
    /// Generate the local node config and start the managed process.
    #[serde(default)]
    start_local: bool,
}

/// POST /api/v1/admin/nodes — creates a Node and returns its token once.
/// With `start_local`, the managed node config is generated from that token
/// and the local process is started immediately.
async fn create(
    State(state): State<ControlApiState>,
    crate::infra::http::extractors::Session { data, .. }: crate::infra::http::extractors::Session,
    Json(body): Json<CreateNodeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.nodes.create";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;

    if body.name.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "name is required".to_owned(),
        });
    }

    let region = body.region.as_deref().unwrap_or("default");
    let region =
        grass_validator::normalize_region(region).map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;

    crate::domain::regions::require(db, &region, OP).await?;
    let token = grass_token::generate_token();
    let node = nodes::create_node(
        db,
        CreateNodeParams {
            name: body.name.trim().to_owned(),
            region: region.clone(),
            token_hash: grass_token::hash_token(&token),
            storage_root: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = crate::infra::http::database(&state, OP)?;
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
                region: &region,
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
                        audits::observe_platform_event(
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

    Ok(ok_response(CreateResponse {
        node: node_view(&node, NodeUsage::default(), None, OffsetDateTime::now_utc()),
        token,
        local_process,
        warnings,
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
struct LocalProcessResponse {
    auto_start: bool,
    managed: bool,
    process: crate::infra::node_manager::ProcessStatus,
}

#[derive(serde::Serialize)]
struct ListResponse {
    nodes: Vec<NodeResponse>,
    local_process: LocalProcessResponse,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    node: NodeResponse,
    token: String,
    local_process: Option<crate::infra::node_manager::ProcessStatus>,
    warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scheduler::NodeUsage;
    use crate::infra::database::entity::NodeStatus;
    use serde_json::json;

    use crate::infra::database::entity::node;
    use time::OffsetDateTime;
    use uuid::Uuid;
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
    fn node_view_reports_capacity_usage_and_overflow() {
        let node = serve_node();
        let usage = NodeUsage {
            cpu_millicores: 1_400,
            memory_mb: 512,
            disk_mb: 1_024,
            deployments: 12,
        };

        let view =
            serde_json::to_value(node_view(&node, usage, None, OffsetDateTime::now_utc())).unwrap();

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
