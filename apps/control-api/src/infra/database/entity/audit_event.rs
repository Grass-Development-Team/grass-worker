use sea_orm::entity::prelude::*;

use super::enums::AuditEventResult;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: AuditEventResult,
    pub reason: Option<String>,
    pub metadata: Json,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::ActorUserId",
        to = "super::user::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    ActorUser,
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActorUser.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
