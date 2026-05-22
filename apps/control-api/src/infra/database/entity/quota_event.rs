use sea_orm::entity::prelude::*;

use super::enums::QuotaEventKind;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "quota_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub dimension: String,
    pub kind: QuotaEventKind,
    pub delta_value: i64,
    pub idempotency_key: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub metadata: Json,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::team::Entity",
        from = "Column::TeamId",
        to = "super::team::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Team,
}
impl Related<super::team::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Team.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
