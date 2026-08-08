use sea_orm::entity::prelude::*;

use super::enums::StorageMigrationObjectStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "storage_migration_objects")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub job_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub object_key: String,
    pub source_size: i64,
    pub status: StorageMigrationObjectStatus,
    pub checksum_sha256: Option<String>,
    pub attempt_count: i32,
    pub last_error: Option<String>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::storage_migration_job::Entity",
        from = "Column::JobId",
        to = "super::storage_migration_job::Column::Id"
    )]
    Job,
}

impl Related<super::storage_migration_job::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Job.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
