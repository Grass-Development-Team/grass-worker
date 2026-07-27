use sea_orm::entity::prelude::*;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "source_credential_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub credential_id: Uuid,
    pub version: i32,
    pub key_id: String,
    pub encrypted_payload: Json,
    pub revoked_at: Option<TimeDateTimeWithTimeZone>,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::source_credential::Entity",
        from = "Column::CredentialId",
        to = "super::source_credential::Column::Id",
        on_delete = "Cascade"
    )]
    Credential,
}

impl ActiveModelBehavior for ActiveModel {}
