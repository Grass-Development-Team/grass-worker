use sea_orm::entity::prelude::*;

use super::enums::{AuditActorType, AuditEventResult, AuditEventVisibility};

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub actor_node_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub actor_type: AuditActorType,
    pub visibility: AuditEventVisibility,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<Uuid>,
    pub result: AuditEventResult,
    pub reason: Option<String>,
    pub metadata: Json,
    pub request_id: Option<Uuid>,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub http_method: Option<String>,
    pub request_path: Option<String>,
    pub status_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub changes: Json,
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
    #[sea_orm(
        belongs_to = "super::node::Entity",
        from = "Column::ActorNodeId",
        to = "super::node::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    ActorNode,
}
impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActorUser.def()
    }
}
impl Related<super::node::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActorNode.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
