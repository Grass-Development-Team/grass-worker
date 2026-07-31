use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "user_notifications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub recipient_user_id: Uuid,
    pub actor_user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub action: String,
    pub project_name: String,
    pub project_slug: String,
    pub actor_label: String,
    pub reason: Option<String>,
    pub target_url: String,
    pub read_at: Option<TimeDateTimeWithTimeZone>,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
