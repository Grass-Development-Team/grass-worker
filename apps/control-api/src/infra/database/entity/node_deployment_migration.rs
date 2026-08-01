use sea_orm::entity::prelude::*;

use super::enums::NodeDeploymentMigrationStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "node_deployment_migrations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub job_id: Uuid,
    pub deployment_id: Uuid,
    pub source_node_id: Uuid,
    pub target_node_id: Uuid,
    pub status: NodeDeploymentMigrationStatus,
    pub error: Option<String>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
    pub ready_at: Option<TimeDateTimeWithTimeZone>,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
