use sea_orm::entity::prelude::*;

use super::enums::TeamKind;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "teams")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub slug: String,
    pub name: String,
    pub avatar_version: Option<Uuid>,
    pub kind: TeamKind,
    pub group_id: Option<Uuid>,
    pub explicit_quota_plan_id: Option<Uuid>,
    pub owner_user_id: Option<Uuid>,
    pub deleted_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::team_group::Entity",
        from = "Column::GroupId",
        to = "super::team_group::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    TeamGroup,
    #[sea_orm(
        belongs_to = "super::quota_plan::Entity",
        from = "Column::ExplicitQuotaPlanId",
        to = "super::quota_plan::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    ExplicitQuotaPlan,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::OwnerUserId",
        to = "super::user::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    OwnerUser,
}

impl Related<super::team_group::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TeamGroup.def()
    }
}

impl Related<super::quota_plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ExplicitQuotaPlan.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OwnerUser.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
