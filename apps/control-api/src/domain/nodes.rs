use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter,
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{NodeStatus, node};

pub struct CreateNodeParams {
    pub name: String,
    pub token_hash: String,
    pub storage_root: Option<String>,
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
        metadata: Set(json!({})),
        last_heartbeat_at: Set(None),
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
}

/// Applies a Node registration: capabilities, version metadata, base URL,
/// and an immediate heartbeat. The first stage forces build and serve on.
pub async fn apply_registration<C: ConnectionTrait>(
    db: &C,
    node: node::Model,
    params: RegisterNodeParams,
) -> anyhow::Result<node::Model> {
    let mut metadata = node.metadata.clone();
    if let Some(map) = metadata.as_object_mut() {
        map.insert("version".to_owned(), json!(params.version));
        map.insert(
            "requested_capabilities".to_owned(),
            json!({ "build": params.build_enabled, "serve": params.serve_enabled }),
        );
    } else {
        metadata = json!({ "version": params.version });
    }

    let mut active: node::ActiveModel = node.into();
    active.name = Set(params.name);
    // First stage: every Node builds and serves.
    active.build_enabled = Set(true);
    active.serve_enabled = Set(true);
    active.build_concurrency = Set(params.build_concurrency.max(1));
    if params.base_url.is_some() {
        active.base_url = Set(params.base_url);
    }
    active.metadata = Set(metadata);
    active.status = Set(NodeStatus::Active);
    active.last_heartbeat_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await.map_err(Into::into)
}

pub async fn record_heartbeat<C: ConnectionTrait>(
    db: &C,
    node: node::Model,
) -> anyhow::Result<node::Model> {
    let mut active: node::ActiveModel = node.into();
    active.status = Set(NodeStatus::Active);
    active.last_heartbeat_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await.map_err(Into::into)
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
            sea_orm::sea_query::Expr::value(NodeStatus::Offline),
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

#[cfg(test)]
mod tests {
    use super::*;

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
            metadata: json!({}),
            last_heartbeat_at: Some(
                OffsetDateTime::now_utc() - time::Duration::seconds(age_seconds),
            ),
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
}
