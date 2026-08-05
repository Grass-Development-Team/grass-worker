use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "codes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub scope: String,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub token_prefix: String,
    pub token_suffix: String,
    pub expires_at: Option<TimeDateTimeWithTimeZone>,
    pub used_at: Option<TimeDateTimeWithTimeZone>,
    pub used_by_user_id: Option<Uuid>,
    pub revoked_at: Option<TimeDateTimeWithTimeZone>,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UsedByUserId",
        to = "super::user::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    UsedByUser,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::CreatedByUserId",
        to = "super::user::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    CreatedByUser,
}

impl ActiveModelBehavior for ActiveModel {}
