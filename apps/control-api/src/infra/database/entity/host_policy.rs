use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "host_policies")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub team_group_id: Option<Uuid>,
    pub quota_plan_id: Option<Uuid>,
    pub max_hosts: i32,
    pub allow_custom_hosts: bool,
    pub allow_auto_assign: bool,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::team_group::Entity",
        from = "Column::TeamGroupId",
        to = "super::team_group::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    TeamGroup,
    #[sea_orm(
        belongs_to = "super::quota_plan::Entity",
        from = "Column::QuotaPlanId",
        to = "super::quota_plan::Column::Id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    QuotaPlan,
}
impl Related<super::team_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TeamGroup.def()
    }
}
impl Related<super::quota_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::QuotaPlan.def()
    }
}
impl ActiveModelBehavior for ActiveModel {}
