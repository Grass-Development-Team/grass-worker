use sea_orm::entity::prelude::*;

use super::enums::NodeDeletionStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "node_deletion_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub node_id: Uuid,
    pub target_node_id: Option<Uuid>,
    pub requested_by_user_id: Option<Uuid>,
    pub status: NodeDeletionStatus,
    pub total_deployments: i32,
    pub migrated_deployments: i32,
    pub active_builds: i32,
    pub error: Option<String>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
    pub completed_at: Option<TimeDateTimeWithTimeZone>,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
