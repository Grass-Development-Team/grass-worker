pub(crate) mod by_provider_id;

use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, QueryOrder};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, IdentityProviderKind, auth_identity_provider},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/identity-providers", axum::routing::get(list).post(create))
        .merge(by_provider_id::router())
}

fn provider_view(provider: &auth_identity_provider::Model) -> IdentityProviderResponse {
    IdentityProviderResponse {
        id: provider.id,
        slug: provider.slug.clone(),
        kind: provider.kind.as_str(),
        name: provider.name.clone(),
        enabled: provider.enabled,
        client_id: provider.client_id.clone(),
        client_secret_configured: true,
        issuer_url: provider.issuer_url.clone(),
        authorization_url: provider.authorization_url.clone(),
        token_url: provider.token_url.clone(),
        userinfo_url: provider.userinfo_url.clone(),
        jwks_url: provider.jwks_url.clone(),
        scopes: provider.scopes.clone(),
        created_at: provider.created_at,
        updated_at: provider.updated_at,
    }
}

pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.identity_providers.list";
    let db = crate::infra::http::database(&state, OP)?;
    let providers = auth_identity_provider::Entity::find()
        .order_by_asc(auth_identity_provider::Column::Name)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(ListResponse {
        providers: providers.iter().map(provider_view).collect::<Vec<_>>(),
    }))
}

#[derive(Clone, Deserialize)]
pub struct IdentityProviderRequest {
    pub slug: String,
    pub template: Option<String>,
    pub kind: Option<String>,
    pub name: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub issuer_url: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

struct ProviderValues {
    kind: IdentityProviderKind,
    issuer_url: Option<String>,
    authorization_url: String,
    token_url: String,
    userinfo_url: Option<String>,
    jwks_url: Option<String>,
    scopes: Vec<String>,
}

fn validate_https_url(value: &str, field: &str, op: &'static str) -> Result<(), AppError> {
    let url = url::Url::parse(value).map_err(|_| AppError::Validation {
        op,
        message: format!("{field} must be a valid HTTPS URL"),
    })?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err(AppError::Validation {
            op,
            message: format!("{field} must be a valid HTTPS URL"),
        });
    }
    Ok(())
}

fn provider_values(
    body: &IdentityProviderRequest,
    op: &'static str,
) -> Result<ProviderValues, AppError> {
    let template = body
        .template
        .as_deref()
        .or(body.kind.as_deref())
        .unwrap_or("custom")
        .trim()
        .to_ascii_lowercase();
    let values = match template.as_str() {
        "google" => ProviderValues {
            kind: IdentityProviderKind::Oidc,
            issuer_url: Some("https://accounts.google.com".to_owned()),
            authorization_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token_url: "https://oauth2.googleapis.com/token".to_owned(),
            userinfo_url: Some("https://openidconnect.googleapis.com/v1/userinfo".to_owned()),
            jwks_url: Some("https://www.googleapis.com/oauth2/v3/certs".to_owned()),
            scopes: vec![
                "openid".to_owned(),
                "email".to_owned(),
                "profile".to_owned(),
            ],
        },
        "apple" => ProviderValues {
            kind: IdentityProviderKind::Oidc,
            issuer_url: Some("https://appleid.apple.com".to_owned()),
            authorization_url: "https://appleid.apple.com/auth/authorize".to_owned(),
            token_url: "https://appleid.apple.com/auth/token".to_owned(),
            userinfo_url: None,
            jwks_url: Some("https://appleid.apple.com/auth/keys".to_owned()),
            scopes: vec!["openid".to_owned(), "email".to_owned(), "name".to_owned()],
        },
        "github" => ProviderValues {
            kind: IdentityProviderKind::Github,
            issuer_url: None,
            authorization_url: "https://github.com/login/oauth/authorize".to_owned(),
            token_url: "https://github.com/login/oauth/access_token".to_owned(),
            userinfo_url: Some("https://api.github.com/user".to_owned()),
            jwks_url: None,
            scopes: vec!["read:user".to_owned(), "user:email".to_owned()],
        },
        "oidc" | "custom" => ProviderValues {
            kind: IdentityProviderKind::Oidc,
            issuer_url: body.issuer_url.clone(),
            authorization_url: body.authorization_url.clone().unwrap_or_default(),
            token_url: body.token_url.clone().unwrap_or_default(),
            userinfo_url: body.userinfo_url.clone(),
            jwks_url: body.jwks_url.clone(),
            scopes: body.scopes.clone().unwrap_or_else(|| {
                vec![
                    "openid".to_owned(),
                    "email".to_owned(),
                    "profile".to_owned(),
                ]
            }),
        },
        _ => {
            return Err(AppError::Validation {
                op,
                message: "identity provider template must be google, apple, github or custom"
                    .to_owned(),
            });
        }
    };
    let slug = body.slug.trim();
    if slug.is_empty()
        || slug.len() > 63
        || !slug.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || !slug
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        || !slug
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    {
        return Err(AppError::Validation {
            op,
            message: "provider slug must contain lowercase letters, numbers and hyphens".to_owned(),
        });
    }
    let name = body.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::Validation {
            op,
            message: "provider name must contain between 1 and 120 characters".to_owned(),
        });
    }
    if body.client_id.trim().is_empty()
        || values.authorization_url.is_empty()
        || values.token_url.is_empty()
        || values.scopes.is_empty()
        || (values.kind == IdentityProviderKind::Oidc
            && (values.issuer_url.is_none() || values.jwks_url.is_none()))
    {
        return Err(AppError::Validation {
            op,
            message: "identity provider endpoints and client credentials are incomplete".to_owned(),
        });
    }
    for (field, endpoint) in [
        ("issuer_url", values.issuer_url.as_deref()),
        ("authorization_url", Some(values.authorization_url.as_str())),
        ("token_url", Some(values.token_url.as_str())),
        ("userinfo_url", values.userinfo_url.as_deref()),
        ("jwks_url", values.jwks_url.as_deref()),
    ] {
        if let Some(endpoint) = endpoint {
            validate_https_url(endpoint, field, op)?;
        }
    }
    Ok(values)
}

fn encrypt_client_secret(
    state: &ControlApiState,
    provider_id: Uuid,
    secret: &str,
) -> Result<serde_json::Value, AppError> {
    let key = state.config.read().unwrap().secrets.secret_key.clone();
    let envelope = grass_crypto::encrypt_secret(
        "platform-secret-v1",
        &crate::domain::authentication::authentication_key(&key),
        secret.as_bytes(),
        format!("grass-identity-provider:v1:{provider_id}").as_bytes(),
    )
    .map_err(|_| AppError::Internal {
        op: "admin.identity_providers.encrypt_secret",
        message: "identity provider secret could not be encrypted".to_owned(),
    })?;
    serde_json::to_value(envelope).map_err(|source| AppError::Internal {
        op: "admin.identity_providers.encrypt_secret",
        message: source.to_string(),
    })
}

pub async fn create(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<IdentityProviderRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.identity_providers.create";
    let values = provider_values(&body, OP)?;
    let secret = body
        .client_secret
        .as_deref()
        .filter(|secret| !secret.is_empty())
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "client_secret is required when creating a provider".to_owned(),
        })?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let now = time::OffsetDateTime::now_utc();
    let id = Uuid::now_v7();
    let provider = auth_identity_provider::ActiveModel {
        id: Set(id),
        slug: Set(body.slug.trim().to_owned()),
        kind: Set(values.kind),
        name: Set(body.name.trim().to_owned()),
        enabled: Set(body.enabled.unwrap_or(true)),
        client_id: Set(body.client_id.trim().to_owned()),
        client_secret_envelope: Set(encrypt_client_secret(&state, id, secret)?),
        issuer_url: Set(values.issuer_url),
        authorization_url: Set(values.authorization_url),
        token_url: Set(values.token_url),
        userinfo_url: Set(values.userinfo_url),
        jwks_url: Set(values.jwks_url),
        scopes: Set(json!(values.scopes)),
        created_by_user_id: Set(Some(data.user_id)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|source| {
        let source = anyhow::Error::from(source);
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "provider slug already exists".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    record_provider_audit(
        db,
        data.user_id,
        "identity_provider.created",
        provider.id,
        &provider.slug,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(CreateResponse {
        provider: provider_view(&provider),
    }))
}

async fn record_provider_audit(
    db: &impl audits::AuditConnection,
    actor_user_id: Uuid,
    action: &str,
    provider_id: Uuid,
    slug: &str,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: None,
            action: action.to_owned(),
            target_type: "identity_provider".to_owned(),
            target_id: Some(provider_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "slug": slug }),
        },
    )
    .await
}

#[derive(serde::Serialize)]
struct IdentityProviderResponse {
    id: uuid::Uuid,
    slug: String,
    kind: &'static str,
    name: String,
    enabled: bool,
    client_id: String,
    client_secret_configured: bool,
    issuer_url: Option<String>,
    authorization_url: String,
    token_url: String,
    userinfo_url: Option<String>,
    jwks_url: Option<String>,
    scopes: serde_json::Value,
    created_at: time::OffsetDateTime,
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    providers: Vec<IdentityProviderResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    provider: IdentityProviderResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_provider() -> IdentityProviderRequest {
        IdentityProviderRequest {
            slug: "custom-provider".to_owned(),
            template: Some("custom".to_owned()),
            kind: None,
            name: "Custom Provider".to_owned(),
            client_id: "client-id".to_owned(),
            client_secret: Some("secret".to_owned()),
            issuer_url: Some("https://identity.example.com".to_owned()),
            authorization_url: Some("https://identity.example.com/authorize".to_owned()),
            token_url: Some("https://identity.example.com/token".to_owned()),
            userinfo_url: Some("https://identity.example.com/userinfo".to_owned()),
            jwks_url: Some("https://identity.example.com/keys".to_owned()),
            scopes: Some(vec!["openid".to_owned(), "email".to_owned()]),
            enabled: Some(true),
        }
    }

    #[test]
    fn custom_provider_requires_https_endpoints() {
        assert!(provider_values(&custom_provider(), "test.provider").is_ok());
        let mut provider = custom_provider();
        provider.token_url = Some("http://identity.example.com/token".to_owned());
        assert!(provider_values(&provider, "test.provider").is_err());
    }

    #[test]
    fn provider_slug_cannot_start_or_end_with_a_hyphen() {
        for slug in ["-provider", "provider-"] {
            let mut provider = custom_provider();
            provider.slug = slug.to_owned();
            assert!(provider_values(&provider, "test.provider").is_err());
        }
    }
}
