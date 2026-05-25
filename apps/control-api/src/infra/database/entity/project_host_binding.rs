use sea_orm::entity::prelude::*;

use super::enums::{HostBindingEnvironment, HostBindingKind, HostBindingStatus};

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "project_host_bindings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub project_id: Uuid,
    pub team_id: Uuid,
    pub host_source_id: Option<Uuid>,
    pub host: String,
    pub kind: HostBindingKind,
    pub environment: HostBindingEnvironment,
    pub status: HostBindingStatus,
    pub failure_reason: Option<String>,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::team::Entity",
        from = "Column::TeamId",
        to = "super::team::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Team,
    #[sea_orm(
        belongs_to = "super::host_source::Entity",
        from = "Column::HostSourceId",
        to = "super::host_source::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    HostSource,
}
impl Related<super::project::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Project.def()
    }
}
impl Related<super::team::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Team.def()
    }
}
impl Related<super::host_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HostSource.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
