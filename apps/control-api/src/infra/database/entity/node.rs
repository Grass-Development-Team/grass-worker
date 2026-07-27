use sea_orm::entity::prelude::*;

use super::enums::NodeStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "nodes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub token_hash: String,
    pub status: NodeStatus,
    pub build_enabled: bool,
    pub serve_enabled: bool,
    pub build_concurrency: i32,
    pub base_url: Option<String>,
    pub work_root: Option<String>,
    pub capacity_cpu_millicores: i64,
    pub capacity_memory_mb: i64,
    pub capacity_disk_mb: i64,
    pub max_deployments: i32,
    pub metadata: Json,
    pub last_heartbeat_at: Option<TimeDateTimeWithTimeZone>,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
