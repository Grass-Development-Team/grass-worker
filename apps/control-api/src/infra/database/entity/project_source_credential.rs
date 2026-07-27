use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "project_source_credentials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub project_id: Uuid,
    pub credential_id: Uuid,
    pub team_id: Uuid,
    pub bound_by_user_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project::Entity",
        from = "Column::ProjectId",
        to = "super::project::Column::Id",
        on_delete = "Cascade"
    )]
    Project,
    #[sea_orm(
        belongs_to = "super::source_credential::Entity",
        from = "Column::CredentialId",
        to = "super::source_credential::Column::Id",
        on_delete = "Cascade"
    )]
    Credential,
}

impl ActiveModelBehavior for ActiveModel {}
