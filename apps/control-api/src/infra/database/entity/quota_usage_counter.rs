use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "quota_usage_counters")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_id: Uuid,
    pub dimension: String,
    pub used_value: i64,
    pub period_start: Option<TimeDateTimeWithTimeZone>,
    pub period_end: Option<TimeDateTimeWithTimeZone>,
    pub updated_at: TimeDateTimeWithTimeZone,
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
