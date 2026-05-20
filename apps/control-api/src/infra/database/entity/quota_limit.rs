use sea_orm::entity::prelude::*;

use super::enums::QuotaPeriod;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "quota_limits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub quota_plan_id: Uuid,
    pub dimension: String,
    pub limit_value: i64,
    pub period: QuotaPeriod,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::quota_plan::Entity",
        from = "Column::QuotaPlanId",
        to = "super::quota_plan::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    QuotaPlan,
}
impl Related<super::quota_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::QuotaPlan.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
