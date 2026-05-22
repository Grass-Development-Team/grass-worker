use sea_orm::entity::prelude::*;

use super::enums::HostProvisionEventStatus;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "host_provision_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub host_binding_id: Uuid,
    pub host_source_id: Option<Uuid>,
    pub status: HostProvisionEventStatus,
    pub operation: String,
    pub provider_request_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub metadata: Json,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project_host_binding::Entity",
        from = "Column::HostBindingId",
        to = "super::project_host_binding::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    HostBinding,
    #[sea_orm(
        belongs_to = "super::host_source::Entity",
        from = "Column::HostSourceId",
        to = "super::host_source::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    HostSource,
}
impl Related<super::project_host_binding::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HostBinding.def()
    }
}
impl Related<super::host_source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::HostSource.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
