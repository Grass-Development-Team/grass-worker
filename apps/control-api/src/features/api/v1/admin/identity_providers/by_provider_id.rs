use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};
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
    axum::Router::new().route(
        "/identity-providers/{provider_id}",
        axum::routing::delete(remove).patch(update),
    )
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

#[derive(Deserialize)]
pub struct UpdateIdentityProviderRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub issuer_url: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub scopes: Option<Vec<String>>,
}

pub async fn update(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(provider_id): Path<Uuid>,
    Json(body): Json<UpdateIdentityProviderRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.identity_providers.update";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let provider = auth_identity_provider::Entity::find_by_id(provider_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "identity provider not found".to_owned(),
        })?;
    let name = body
        .name
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| provider.name.clone());
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::Validation {
            op: OP,
            message: "provider name must contain between 1 and 120 characters".to_owned(),
        });
    }
    let client_id = body
        .client_id
        .map(|value| value.trim().to_owned())
        .unwrap_or_else(|| provider.client_id.clone());
    let issuer_url = body.issuer_url.or_else(|| provider.issuer_url.clone());
    let authorization_url = body
        .authorization_url
        .unwrap_or_else(|| provider.authorization_url.clone());
    let token_url = body.token_url.unwrap_or_else(|| provider.token_url.clone());
    let userinfo_url = body.userinfo_url.or_else(|| provider.userinfo_url.clone());
    let jwks_url = body.jwks_url.or_else(|| provider.jwks_url.clone());
    let scopes = body.scopes.unwrap_or_else(|| {
        provider
            .scopes
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|scope| scope.as_str().map(str::to_owned))
            .collect()
    });
    if client_id.is_empty()
        || authorization_url.trim().is_empty()
        || token_url.trim().is_empty()
        || scopes.is_empty()
        || scopes.iter().any(|scope| scope.trim().is_empty())
        || (provider.kind == IdentityProviderKind::Oidc
            && (issuer_url.is_none() || jwks_url.is_none()))
    {
        return Err(AppError::Validation {
            op: OP,
            message: "identity provider endpoints and client credentials are incomplete".to_owned(),
        });
    }
    for (field, endpoint) in [
        ("issuer_url", issuer_url.as_deref()),
        ("authorization_url", Some(authorization_url.as_str())),
        ("token_url", Some(token_url.as_str())),
        ("userinfo_url", userinfo_url.as_deref()),
        ("jwks_url", jwks_url.as_deref()),
    ] {
        if let Some(endpoint) = endpoint {
            validate_https_url(endpoint, field, OP)?;
        }
    }
    let mut active: auth_identity_provider::ActiveModel = provider.into();
    active.name = Set(name);
    active.client_id = Set(client_id);
    active.issuer_url = Set(issuer_url);
    active.authorization_url = Set(authorization_url);
    active.token_url = Set(token_url);
    active.userinfo_url = Set(userinfo_url);
    active.jwks_url = Set(jwks_url);
    active.scopes = Set(json!(scopes));
    if let Some(enabled) = body.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(secret) = body.client_secret.filter(|value| !value.is_empty()) {
        active.client_secret_envelope = Set(encrypt_client_secret(&state, provider_id, &secret)?);
    }
    active.updated_at = Set(time::OffsetDateTime::now_utc());
    let provider = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    record_provider_audit(
        db,
        data.user_id,
        "identity_provider.updated",
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
    Ok(ok_response(UpdateResponse {
        provider: provider_view(&provider),
    }))
}

pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(provider_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.identity_providers.remove";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let result = auth_identity_provider::Entity::delete_by_id(provider_id)
        .exec(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if result.rows_affected == 0 {
        return Err(AppError::NotFound {
            op: OP,
            message: "identity provider not found".to_owned(),
        });
    }
    record_provider_audit(
        db,
        data.user_id,
        "identity_provider.deleted",
        provider_id,
        "",
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
    Ok(ok_response(RemoveResponse { deleted: true }))
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
struct UpdateResponse {
    provider: IdentityProviderResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    deleted: bool,
}
