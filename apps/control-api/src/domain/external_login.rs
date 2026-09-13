//! Shared identity-provider flow state and provider lookup.
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::settings,
    infra::{database::entity::auth_identity_provider, error::AppError},
};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct AuthorizationFlow {
    pub(crate) provider_id: Uuid,
    pub(crate) nonce: String,
    pub(crate) pkce_verifier: String,
    pub(crate) return_to: String,
    pub(crate) registration_code: Option<String>,
    pub(crate) redirect_uri: String,
}

pub(crate) async fn configured_site_url(
    db: &sea_orm::DatabaseConnection,
    op: &'static str,
) -> Result<String, AppError> {
    settings::get_setting(db, "site.url")
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned))
        .ok_or_else(|| AppError::Internal {
            op,
            message: "site.url is not configured".to_owned(),
        })
}

pub(crate) fn flow_key(state: &str) -> String {
    format!("auth:oauth:flow:{}", grass_token::hash_token(state))
}

pub(crate) async fn provider_by_slug(
    db: &sea_orm::DatabaseConnection,
    slug: &str,
    op: &'static str,
) -> Result<auth_identity_provider::Model, AppError> {
    auth_identity_provider::Entity::find()
        .filter(auth_identity_provider::Column::Slug.eq(slug))
        .filter(auth_identity_provider::Column::Enabled.eq(true))
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "identity provider not found".to_owned(),
        })
}
