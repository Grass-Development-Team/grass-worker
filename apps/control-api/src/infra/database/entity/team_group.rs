use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "team_groups")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub quota_plan_id: Option<Uuid>,
    pub is_default: bool,
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
        on_delete = "SetNull"
    )]
    QuotaPlan,
}

impl Related<super::quota_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::QuotaPlan.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
