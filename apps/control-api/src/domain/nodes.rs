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
