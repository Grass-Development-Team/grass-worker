use sea_orm::entity::prelude::*;

use super::enums::StorageMigrationStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "storage_migration_jobs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub status: StorageMigrationStatus,
    pub source_config: Json,
    pub source_credentials: Option<Json>,
    pub target_config: Json,
    pub target_credentials: Option<Json>,
    pub copied_objects: i64,
    pub copied_bytes: i64,
    pub total_objects: Option<i64>,
    pub total_bytes: Option<i64>,
    pub last_error: Option<String>,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub started_at: Option<TimeDateTimeWithTimeZone>,
    pub finished_at: Option<TimeDateTimeWithTimeZone>,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
