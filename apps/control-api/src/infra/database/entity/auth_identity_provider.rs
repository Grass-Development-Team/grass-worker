use sea_orm::entity::prelude::*;

use super::enums::IdentityProviderKind;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "auth_identity_providers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub slug: String,
    pub kind: IdentityProviderKind,
    pub name: String,
    pub enabled: bool,
    pub client_id: String,
    pub client_secret_envelope: Json,
    pub issuer_url: Option<String>,
    pub authorization_url: String,
    pub token_url: String,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes: Json,
    pub created_by_user_id: Option<Uuid>,
    pub created_at: TimeDateTimeWithTimeZone,
    pub updated_at: TimeDateTimeWithTimeZone,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
