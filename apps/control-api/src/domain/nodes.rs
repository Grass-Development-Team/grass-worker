use grass_node_protocol::NodeResources;
use ring::hmac;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QuerySelect,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{NodeConfigSyncStatus, NodeStatus, node};

pub fn gateway_token(secret: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
    hex::encode(hmac::sign(&key, b"grass-node-gateway-v1").as_ref())
}

pub struct CreateNodeParams {
    pub name: String,
    pub token_hash: String,
    pub storage_root: Option<String>,
}

fn status_after_node_activity(current: &NodeStatus) -> NodeStatus {
    if matches!(current, NodeStatus::Draining) {
        NodeStatus::Draining
    } else {
        NodeStatus::Active
    }
}

pub async fn create_node(
    db: &DatabaseConnection,
    params: CreateNodeParams,
) -> anyhow::Result<node::Model> {
    let now = OffsetDateTime::now_utc();
    node::ActiveModel {
        id: Set(Uuid::now_v7()),
        name: Set(params.name.clone()),
        token_hash: Set(params.token_hash),
        status: Set(NodeStatus::Pending),
        build_enabled: Set(true),
        serve_enabled: Set(true),
        build_concurrency: Set(2),
        base_url: Set(None),
        work_root: Set(params
            .storage_root
            .or_else(|| Some("/data/node".to_string()))),
        capacity_cpu_millicores: Set(0),
        capacity_memory_mb: Set(0),
        capacity_disk_mb: Set(0),
        max_deployments: Set(10),
        metadata: Set(json!({})),
        last_heartbeat_at: Set(None),
        desired_config: Set(None),
        desired_config_revision: Set(0),
        effective_config: Set(None),
        effective_config_revision: Set(0),
        config_sync_status: Set(NodeConfigSyncStatus::Pending),
        config_sync_error: Set(None),
        node_token_configured: Set(false),
        config_updated_at: Set(None),
        config_applied_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|e| anyhow::anyhow!("failed to create node: {e}"))
}

pub async fn any_node_exists(db: &DatabaseConnection) -> anyhow::Result<bool> {
    node::Entity::find()
        .filter(node::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map(|o| o.is_some())
        .map_err(|e| anyhow::anyhow!("failed to check nodes existence: {e}"))
}

pub async fn update_work_roots<C: ConnectionTrait>(db: &C, work_root: &str) -> anyhow::Result<()> {
    node::Entity::update_many()
        .col_expr(
            node::Column::WorkRoot,
            sea_orm::sea_query::Expr::value(work_root),
        )
        .filter(node::Column::DeletedAt.is_null())
        .exec(db)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("failed to update node work roots: {e}"))
}

pub async fn find_by_token_hash<C: ConnectionTrait>(
    db: &C,
    token_hash: &str,
) -> anyhow::Result<Option<node::Model>> {
    node::Entity::find()
        .filter(node::Column::TokenHash.eq(token_hash))
        .filter(node::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn get_by_id<C: ConnectionTrait>(
    db: &C,
    node_id: Uuid,
) -> anyhow::Result<Option<node::Model>> {
    node::Entity::find()
        .filter(node::Column::Id.eq(node_id))
        .filter(node::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn get_by_id_for_update<C: ConnectionTrait>(
    db: &C,
    node_id: Uuid,
) -> anyhow::Result<Option<node::Model>> {
    node::Entity::find_by_id(node_id)
        .filter(node::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn list<C: ConnectionTrait>(db: &C) -> anyhow::Result<Vec<node::Model>> {
    use sea_orm::QueryOrder;

    node::Entity::find()
        .filter(node::Column::DeletedAt.is_null())
        .order_by_asc(node::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub struct RegisterNodeParams {
    pub name: String,
    pub version: String,
    pub build_enabled: bool,
    pub serve_enabled: bool,
    pub build_concurrency: i32,
    pub base_url: Option<String>,
    pub resources: Option<NodeResources>,
    pub config_revision: u64,
    pub effective_config: Option<grass_node_protocol::NodeConfiguration>,
    pub node_token_configured: bool,
}

struct RegistrationConfigState {
    desired: Option<serde_json::Value>,
    desired_revision: i64,
    effective: Option<serde_json::Value>,
    effective_revision: i64,
    status: NodeConfigSyncStatus,
    error: Option<String>,
    node_token_configured: bool,
    updated_at: Option<OffsetDateTime>,
    applied_at: Option<OffsetDateTime>,
}

fn registration_config_state(
    node: &node::Model,
    revision: u64,
    effective: Option<&grass_node_protocol::NodeConfiguration>,
    node_token_configured: bool,
    now: OffsetDateTime,
) -> anyhow::Result<RegistrationConfigState> {
    let Some(effective) = effective else {
        return Ok(RegistrationConfigState {
            desired: node.desired_config.clone(),
            desired_revision: node.desired_config_revision,
            effective: node.effective_config.clone(),
            effective_revision: node.effective_config_revision,
            status: node.config_sync_status.clone(),
            error: node.config_sync_error.clone(),
            node_token_configured: node.node_token_configured,
            updated_at: node.config_updated_at,
            applied_at: node.config_applied_at,
        });
    };
    let revision = i64::try_from(revision)
        .map_err(|_| anyhow::anyhow!("config revision exceeds the supported range"))?;
    let effective = serde_json::to_value(effective)?;
    if node.desired_config.is_none() {
        return Ok(RegistrationConfigState {
            desired: Some(effective.clone()),
            desired_revision: revision,
            effective: Some(effective),
            effective_revision: revision,
            status: NodeConfigSyncStatus::Applied,
            error: None,
            node_token_configured,
            updated_at: Some(now),
            applied_at: Some(now),
        });
    }

    let revision_matches = revision == node.desired_config_revision;
    let configuration_matches = node.desired_config.as_ref() == Some(&effective);
    let (status, error, applied_at) = if revision_matches && configuration_matches {
        (NodeConfigSyncStatus::Applied, None, Some(now))
    } else if revision_matches {
        (
            NodeConfigSyncStatus::Failed,
            Some("effective configuration differs from desired revision".to_owned()),
            node.config_applied_at,
        )
    } else {
        (NodeConfigSyncStatus::Pending, None, node.config_applied_at)
    };
    Ok(RegistrationConfigState {
        desired: node.desired_config.clone(),
        desired_revision: node.desired_config_revision,
        effective: Some(effective),
        effective_revision: revision,
        status,
        error,
        node_token_configured,
        updated_at: node.config_updated_at,
        applied_at,
    })
}

/// Applies a Node registration: capabilities, version metadata, base URL,
/// initial Serve capacity, and an immediate heartbeat.
pub async fn apply_registration<C: ConnectionTrait>(
    db: &C,
    node: node::Model,
    params: RegisterNodeParams,
) -> anyhow::Result<node::Model> {
    let next_status = status_after_node_activity(&node.status);
    let config_state = registration_config_state(
        &node,
        params.config_revision,
        params.effective_config.as_ref(),
        params.node_token_configured,
        OffsetDateTime::now_utc(),
    )?;
    let first_resource_report = node.metadata.get("reported_resources").is_none();
    let mut metadata = node.metadata.clone();
    if let Some(map) = metadata.as_object_mut() {
        map.insert("version".to_owned(), json!(params.version));
        map.insert(
            "reported_capabilities".to_owned(),
            json!({ "build": params.build_enabled, "serve": params.serve_enabled }),
        );
        map.insert("reported_resources".to_owned(), json!(params.resources));
    } else {
        metadata = json!({
            "version": params.version,
            "reported_capabilities": {
                "build": params.build_enabled,
                "serve": params.serve_enabled,
            },
            "reported_resources": params.resources,
        });
    }

    let resources = match (params.serve_enabled, params.resources) {
        (true, Some(resources)) => Some((
            i64::try_from(resources.cpu_millicores)?,
            i64::try_from(resources.memory_mb)?,
            i64::try_from(resources.disk_mb)?,
            i32::try_from(resources.max_deployments)?,
        )),
        (true, None) => anyhow::bail!("serve-capable node registration is missing resources"),
        (false, _) => None,
    };
    let initialize_cpu = node.capacity_cpu_millicores == 0;
    let initialize_memory = node.capacity_memory_mb == 0;
    let initialize_disk = node.capacity_disk_mb == 0;
    let initialize_deployments = first_resource_report;
    let mut active: node::ActiveModel = node.into();
    active.name = Set(params.name);
    active.build_enabled = Set(params.build_enabled);
    active.serve_enabled = Set(params.serve_enabled);
    active.build_concurrency = Set(if params.build_enabled {
        params.build_concurrency
    } else {
        0
    });
    active.base_url = Set(params.serve_enabled.then_some(params.base_url).flatten());
    if let Some((cpu, memory, disk, deployments)) = resources {
        if initialize_cpu {
            active.capacity_cpu_millicores = Set(cpu);
        }
        if initialize_memory {
            active.capacity_memory_mb = Set(memory);
        }
        if initialize_disk {
            active.capacity_disk_mb = Set(disk);
        }
        if initialize_deployments {
            active.max_deployments = Set(deployments);
        }
    }
    active.metadata = Set(metadata);
    active.desired_config = Set(config_state.desired);
    active.desired_config_revision = Set(config_state.desired_revision);
    active.effective_config = Set(config_state.effective);
    active.effective_config_revision = Set(config_state.effective_revision);
    active.config_sync_status = Set(config_state.status);
    active.config_sync_error = Set(config_state.error);
    active.node_token_configured = Set(config_state.node_token_configured);
    active.config_updated_at = Set(config_state.updated_at);
    active.config_applied_at = Set(config_state.applied_at);
    active.status = Set(next_status);
    active.last_heartbeat_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await.map_err(Into::into)
}

pub async fn record_heartbeat<C: ConnectionTrait>(
    db: &C,
    node: node::Model,
    heartbeat: &grass_node_protocol::HeartbeatRequest,
) -> anyhow::Result<node::Model> {
    let next_status = status_after_node_activity(&node.status);
    let effective_revision = i64::try_from(heartbeat.effective_config_revision)
        .map_err(|_| anyhow::anyhow!("effective config revision exceeds the supported range"))?;
    let (sync_status, sync_error) = config_sync_state_for_heartbeat(&node, heartbeat);
    let applied = sync_status == NodeConfigSyncStatus::Applied;
    let mut active: node::ActiveModel = node.into();
    active.status = Set(next_status);
    active.last_heartbeat_at = Set(Some(OffsetDateTime::now_utc()));
    active.effective_config_revision = Set(effective_revision);
    active.config_sync_status = Set(sync_status);
    active.config_sync_error = Set(sync_error);
    if applied {
        active.config_applied_at = Set(Some(OffsetDateTime::now_utc()));
    }
    active.update(db).await.map_err(Into::into)
}

pub fn config_sync_state_for_heartbeat(
    node: &node::Model,
    heartbeat: &grass_node_protocol::HeartbeatRequest,
) -> (NodeConfigSyncStatus, Option<String>) {
    let desired_revision = u64::try_from(node.desired_config_revision).unwrap_or_default();
    if node.desired_config.is_some() && heartbeat.effective_config_revision == desired_revision {
        return (NodeConfigSyncStatus::Applied, None);
    }
    if heartbeat.applying_config_revision == Some(desired_revision) {
        if let Some(error) = heartbeat
            .config_apply_error
            .as_deref()
            .map(str::trim)
            .filter(|error| !error.is_empty())
        {
            return (
                NodeConfigSyncStatus::Failed,
                Some(error.chars().take(2_000).collect()),
            );
        }
        return (NodeConfigSyncStatus::Applying, None);
    }
    (NodeConfigSyncStatus::Pending, None)
}

pub fn desired_config_for_heartbeat(
    node: &node::Model,
    heartbeat: &grass_node_protocol::HeartbeatRequest,
) -> anyhow::Result<(Option<u64>, Option<grass_node_protocol::NodeConfiguration>)> {
    let desired_revision = u64::try_from(node.desired_config_revision)
        .map_err(|_| anyhow::anyhow!("stored desired config revision is invalid"))?;
    if heartbeat.effective_config_revision == desired_revision
        || (heartbeat.applying_config_revision == Some(desired_revision)
            && heartbeat.config_apply_error.is_none())
    {
        return Ok((None, None));
    }
    let Some(desired) = node.desired_config.clone() else {
        return Ok((None, None));
    };
    let desired = serde_json::from_value(desired)
        .map_err(|error| anyhow::anyhow!("stored desired Node config is invalid: {error}"))?;
    Ok((Some(desired_revision), Some(desired)))
}

/// Marks Active nodes with a heartbeat older than the threshold as Offline.
/// Returns how many nodes were flipped.
pub async fn mark_stale_offline<C: ConnectionTrait>(
    db: &C,
    stale_after_seconds: i64,
) -> anyhow::Result<u64> {
    let threshold = OffsetDateTime::now_utc() - time::Duration::seconds(stale_after_seconds);
    let result = node::Entity::update_many()
        .col_expr(
            node::Column::Status,
            sea_orm::ActiveEnum::as_enum(&NodeStatus::Offline),
        )
        .filter(node::Column::Status.eq(NodeStatus::Active))
        .filter(node::Column::DeletedAt.is_null())
        .filter(
            sea_orm::Condition::any()
                .add(node::Column::LastHeartbeatAt.lt(threshold))
                .add(node::Column::LastHeartbeatAt.is_null()),
        )
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

pub async fn replace_token_hash<C: ConnectionTrait>(
    db: &C,
    node: node::Model,
    token_hash: String,
) -> anyhow::Result<node::Model> {
    let mut active: node::ActiveModel = node.into();
    active.token_hash = Set(token_hash);
    active.update(db).await.map_err(Into::into)
}

/// Node health as shown in the admin console: healthy when the heartbeat is
/// fresh enough.
pub fn is_healthy(node: &node::Model, now: OffsetDateTime, stale_after_seconds: i64) -> bool {
    matches!(node.status, NodeStatus::Active)
        && node
            .last_heartbeat_at
            .is_some_and(|at| (now - at).whole_seconds() <= stale_after_seconds)
}

pub fn status_value(status: &NodeStatus) -> &'static str {
    match status {
        NodeStatus::Pending => "pending",
        NodeStatus::Active => "active",
        NodeStatus::Draining => "draining",
        NodeStatus::Offline => "offline",
        NodeStatus::Disabled => "disabled",
    }
}

pub fn config_sync_status_value(status: &NodeConfigSyncStatus) -> &'static str {
    match status {
        NodeConfigSyncStatus::Pending => "pending",
        NodeConfigSyncStatus::Applying => "applying",
        NodeConfigSyncStatus::Applied => "applied",
        NodeConfigSyncStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported_config(concurrency: u16) -> grass_node_protocol::NodeConfiguration {
        serde_json::from_value(json!({
            "node": {
                "id": "node-a",
                "control_api": "https://control.example.test",
                "work_root": "/data/node",
                "capabilities": { "build": true, "serve": true }
            },
            "build": {
                "concurrency": concurrency,
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
            "security": { "private_repository_targets": [] },
            "development": { "verbose_build_log": false },
            "log": { "level": "info", "format": "pretty" }
        }))
        .unwrap()
    }

    fn node_with_heartbeat(status: NodeStatus, age_seconds: i64) -> node::Model {
        node::Model {
            id: Uuid::nil(),
            name: "test".to_owned(),
            token_hash: String::new(),
            status,
            build_enabled: true,
            serve_enabled: true,
            build_concurrency: 1,
            base_url: None,
            work_root: None,
            capacity_cpu_millicores: 0,
            capacity_memory_mb: 0,
            capacity_disk_mb: 0,
            max_deployments: 10,
            metadata: json!({}),
            last_heartbeat_at: Some(
                OffsetDateTime::now_utc() - time::Duration::seconds(age_seconds),
            ),
            desired_config: None,
            desired_config_revision: 0,
            effective_config: None,
            effective_config_revision: 0,
            config_sync_status: NodeConfigSyncStatus::Pending,
            config_sync_error: None,
            node_token_configured: false,
            config_updated_at: None,
            config_applied_at: None,
            deleted_at: None,
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        }
    }

    #[test]
    fn nodes_with_fresh_heartbeats_are_healthy() {
        let now = OffsetDateTime::now_utc();
        assert!(is_healthy(
            &node_with_heartbeat(NodeStatus::Active, 10),
            now,
            90
        ));
        assert!(!is_healthy(
            &node_with_heartbeat(NodeStatus::Active, 120),
            now,
            90
        ));
        assert!(!is_healthy(
            &node_with_heartbeat(NodeStatus::Offline, 10),
            now,
            90
        ));
        assert!(!is_healthy(
            &node_with_heartbeat(NodeStatus::Disabled, 10),
            now,
            90
        ));
    }

    #[test]
    fn heartbeat_config_sync_transitions_cover_all_four_states() {
        let mut node = node_with_heartbeat(NodeStatus::Active, 0);
        node.desired_config = Some(json!({ "node": { "id": "node-a" } }));
        node.desired_config_revision = 3;
        let mut heartbeat = grass_node_protocol::HeartbeatRequest {
            active_builds: 0,
            effective_config_revision: 2,
            applying_config_revision: None,
            config_apply_error: None,
        };

        let (status, error) = config_sync_state_for_heartbeat(&node, &heartbeat);
        assert_eq!(status, NodeConfigSyncStatus::Pending);
        assert!(error.is_none());

        heartbeat.applying_config_revision = Some(3);
        let (status, error) = config_sync_state_for_heartbeat(&node, &heartbeat);
        assert_eq!(status, NodeConfigSyncStatus::Applying);
        assert!(error.is_none());

        heartbeat.config_apply_error = Some("configuration file is read-only".to_owned());
        let (status, error) = config_sync_state_for_heartbeat(&node, &heartbeat);
        assert_eq!(status, NodeConfigSyncStatus::Failed);
        assert_eq!(error.as_deref(), Some("configuration file is read-only"));

        heartbeat.effective_config_revision = 3;
        let (status, error) = config_sync_state_for_heartbeat(&node, &heartbeat);
        assert_eq!(status, NodeConfigSyncStatus::Applied);
        assert!(error.is_none());
    }

    #[test]
    fn registration_seeds_desired_config_and_rejects_revision_only_matches() {
        let node = node_with_heartbeat(NodeStatus::Pending, 0);
        let effective = reported_config(2);

        let initial =
            registration_config_state(&node, 0, Some(&effective), true, OffsetDateTime::UNIX_EPOCH)
                .unwrap();
        assert_eq!(initial.status, NodeConfigSyncStatus::Applied);
        assert_eq!(initial.desired, initial.effective);
        assert_eq!(initial.desired_revision, 0);
        assert_eq!(initial.effective_revision, 0);
        assert!(initial.node_token_configured);

        let mut configured = node;
        configured.desired_config = Some(serde_json::to_value(reported_config(4)).unwrap());
        configured.desired_config_revision = 3;
        let mismatch = registration_config_state(
            &configured,
            3,
            Some(&effective),
            true,
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(mismatch.status, NodeConfigSyncStatus::Failed);
        assert_eq!(
            mismatch.error.as_deref(),
            Some("effective configuration differs from desired revision")
        );
    }

    #[test]
    fn heartbeat_only_returns_desired_config_when_revision_differs() {
        let desired = reported_config(4);
        let mut node = node_with_heartbeat(NodeStatus::Active, 0);
        node.desired_config = Some(serde_json::to_value(&desired).unwrap());
        node.desired_config_revision = 3;
        let mut heartbeat = grass_node_protocol::HeartbeatRequest {
            active_builds: 0,
            effective_config_revision: 2,
            applying_config_revision: None,
            config_apply_error: None,
        };

        let (revision, configuration) = desired_config_for_heartbeat(&node, &heartbeat).unwrap();
        assert_eq!(revision, Some(3));
        assert_eq!(configuration, Some(desired));

        heartbeat.effective_config_revision = 3;
        let (revision, configuration) = desired_config_for_heartbeat(&node, &heartbeat).unwrap();
        assert!(revision.is_none());
        assert!(configuration.is_none());
    }

    #[test]
    fn node_activity_never_cancels_an_administrative_drain() {
        assert_eq!(
            status_after_node_activity(&NodeStatus::Draining),
            NodeStatus::Draining
        );
        assert_eq!(
            status_after_node_activity(&NodeStatus::Offline),
            NodeStatus::Active
        );
    }
}
