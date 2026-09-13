use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ColumnTrait, EntityTrait, QueryFilter, QuerySelect, TransactionTrait, sea_query::LockType,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::hosts::{self, HostSourceError, UpdateHostSourceParams},
    infra::{
        database::entity::{HostSourceKind, host_source},
        error::{AppError, ok_response},
        host_provision::{self, cloudflare, credentials, dnspod, route53},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/host-sources/{source_id}",
        axum::routing::delete(remove).patch(update),
    )
}

fn source_view(source: &host_source::Model) -> HostSourceResponse {
    HostSourceResponse {
        id: source.id,
        kind: match source.kind {
            HostSourceKind::Wildcard => "wildcard",
            HostSourceKind::DnsProvider => "dns_provider",
            HostSourceKind::Manual => "manual",
        },
        label: source.label.clone(),
        base_domain: source.base_domain.clone(),
        region: source.region.clone(),
        enabled: source.enabled,
        allows_auto_assign: source.allows_auto_assign,
        is_default: source.is_default,
        provider: source.provider.clone(),
        config_keys: credentials::config_keys(&source.config),
        created_at: source.created_at,
    }
}

fn map_source_error(error: HostSourceError, op: &'static str) -> AppError {
    match error {
        HostSourceError::DuplicateDefault => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        HostSourceError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
    }
}

/// DNS provider sources must name a supported provider and carry a config
/// the matching client can actually use; failing early keeps broken
/// credentials out of the provisioning path.
fn validate_dns_provider_source(
    provider: Option<&str>,
    base_domain: &str,
    config: &serde_json::Value,
    op: &'static str,
) -> Result<(), AppError> {
    match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(name) if name.eq_ignore_ascii_case(cloudflare::PROVIDER_NAME) => {
            cloudflare::CloudflareConfig::from_json(config)
                .map(|_| ())
                .map_err(|message| AppError::Validation {
                    op,
                    message: format!("cloudflare config: {message}"),
                })
        }
        Some(name) if name.eq_ignore_ascii_case(dnspod::PROVIDER_NAME) => {
            dnspod::DnsPodConfig::from_json(base_domain, config)
                .map(|_| ())
                .map_err(|message| AppError::Validation {
                    op,
                    message: format!("dnspod config: {message}"),
                })
        }
        Some(name) if name.eq_ignore_ascii_case(route53::PROVIDER_NAME) => {
            route53::Route53Config::from_json(config)
                .map(|_| ())
                .map_err(|message| AppError::Validation {
                    op,
                    message: format!("route53 config: {message}"),
                })
        }
        Some(other) => Err(AppError::Validation {
            op,
            message: format!(
                "provider '{other}' is not supported for dns_provider sources (supported: {})",
                host_provision::supported_provider_names()
            ),
        }),
        None => Err(AppError::Validation {
            op,
            message: format!(
                "dns_provider sources require a provider (supported: {})",
                host_provision::supported_provider_names()
            ),
        }),
    }
}

/// Shallow-merges a config patch into the stored config: explicit `null`
/// removes a key, nonempty values replace it, omitted/blank keys stay untouched.
/// This lets operators update one field without resending credentials.
fn merge_config(
    existing: &serde_json::Value,
    patch: serde_json::Value,
    op: &'static str,
) -> Result<serde_json::Value, AppError> {
    let Some(patch) = patch.as_object() else {
        return Err(AppError::Validation {
            op,
            message: "config must be a JSON object".to_owned(),
        });
    };
    if patch.contains_key("_format") {
        return Err(AppError::Validation {
            op,
            message: "config must contain provider fields, not an encrypted envelope".to_owned(),
        });
    }
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    for (key, value) in patch {
        if value.is_null() {
            merged.remove(key);
        } else if value.as_str().is_some_and(|value| value.trim().is_empty()) {
            continue;
        } else {
            merged.insert(key.clone(), value.clone());
        }
    }
    Ok(serde_json::Value::Object(merged))
}

#[derive(Deserialize)]
pub struct UpdateHostSourceRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub allows_auto_assign: Option<bool>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

/// PATCH /api/v1/admin/host-sources/{source_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Path(source_id): Path<Uuid>,
    Json(body): Json<UpdateHostSourceRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.update";
    let db = crate::infra::http::database(&state, OP)?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let source = host_source::Entity::find_by_id(source_id)
        .filter(host_source::Column::DeletedAt.is_null())
        .lock(LockType::Update)
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host source not found".to_owned(),
        })?;

    let provider_patch = body.provider.map(|provider| {
        Some(provider.trim().to_ascii_lowercase()).filter(|provider| !provider.is_empty())
    });
    let region = body
        .region
        .map(|value| grass_validator::normalize_region(&value))
        .transpose()
        .map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;
    if let Some(region) = region.as_deref() {
        crate::domain::regions::require(db, region, OP).await?;
    }
    let key = credentials::encryption_key(&state.config.read().unwrap().secrets.secret_key);
    let plaintext = credentials::decrypt_config(
        &key,
        &source.base_domain,
        source.provider.as_deref(),
        &source.config,
    )
    .map_err(|_| AppError::Internal {
        op: OP,
        message: "host source credentials could not be authenticated".to_owned(),
    })?;
    let config_patch = body
        .config
        .map(|patch| merge_config(&plaintext, patch, OP))
        .transpose()?;
    let effective_provider = provider_patch
        .clone()
        .unwrap_or_else(|| source.provider.clone());
    let effective_config = config_patch.as_ref().unwrap_or(&plaintext);
    if source.kind == HostSourceKind::DnsProvider
        && (provider_patch.is_some() || config_patch.is_some())
    {
        validate_dns_provider_source(
            effective_provider.as_deref(),
            &source.base_domain,
            effective_config,
            OP,
        )?;
    }
    let encrypted_config = credentials::encrypt_config(
        &key,
        &source.base_domain,
        effective_provider.as_deref(),
        effective_config,
    )
    .map_err(|_| AppError::Internal {
        op: OP,
        message: "host source credentials could not be encrypted".to_owned(),
    })?;

    let source = hosts::update_source(
        &transaction,
        source,
        UpdateHostSourceParams {
            label: body.label.filter(|label| !label.trim().is_empty()),
            region,
            enabled: body.enabled,
            allows_auto_assign: body.allows_auto_assign,
            is_default: body.is_default,
            provider: provider_patch,
            config: Some(encrypted_config),
        },
    )
    .await
    .map_err(|error| map_source_error(error, OP))?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(UpdateResponse {
        source: source_view(&source),
    }))
}

/// DELETE /api/v1/admin/host-sources/{source_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Path(source_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.remove";
    let db = crate::infra::http::database(&state, OP)?;

    let source = hosts::get_source_by_id(db, source_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host source not found".to_owned(),
        })?;

    hosts::soft_delete_source(db, source)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(RemoveResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct HostSourceResponse {
    id: uuid::Uuid,
    kind: &'static str,
    label: String,
    base_domain: String,
    region: String,
    enabled: bool,
    allows_auto_assign: bool,
    is_default: bool,
    provider: Option<String>,
    config_keys: Vec<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    source: HostSourceResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    ok: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::response::IntoResponse;

    use crate::infra::database::entity::HostSourceKind;
    use crate::infra::database::entity::host_source;
    use crate::infra::host_provision::credentials;
    use crate::state::ControlApiState;
    use axum::Json;
    use axum::body::to_bytes;
    use axum::extract::Path;
    use axum::extract::State;
    use sea_orm::DbBackend;
    use sea_orm::MockDatabase;
    use serde_json::json;
    use uuid::Uuid;
    fn config() -> serde_json::Value {
        json!({"api_token": "private-test-token", "zone_id": "test-zone", "record_type": "CNAME", "record_value": "entry.example.com"})
    }

    #[test]
    fn blank_fields_preserve_secrets_and_explicit_null_removes_optional_values() {
        let original = config();
        let merged = merge_config(
            &original,
            json!({"api_token": "  ", "record_value": "new.example.com", "ttl": null}),
            "test",
        )
        .unwrap();
        assert_eq!(merged["api_token"], original["api_token"]);
        assert_eq!(merged["zone_id"], original["zone_id"]);
        assert_eq!(merged["record_value"], "new.example.com");
        assert!(merge_config(&original, json!({"_format": "client-envelope"}), "test").is_err());
        let incomplete = merge_config(&original, json!({"api_token": null}), "test").unwrap();
        assert!(
            validate_dns_provider_source(Some("cloudflare"), "example.com", &incomplete, "test")
                .is_err()
        );
    }

    fn source(config: serde_json::Value) -> host_source::Model {
        host_source::Model {
            id: Uuid::now_v7(),
            kind: HostSourceKind::DnsProvider,
            label: "test".to_owned(),
            base_domain: "example.com".to_owned(),
            region: "default".to_owned(),
            enabled: true,
            allows_auto_assign: true,
            is_default: false,
            provider: Some("cloudflare".to_owned()),
            config,
            deleted_at: None,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[tokio::test]
    async fn metadata_update_upgrades_legacy_config_without_exposing_values() {
        let mut runtime_config = crate::infra::config::ControlApiConfig::default();
        runtime_config.secrets.secret_key = "test-platform-key".to_owned();
        let key = credentials::encryption_key(&runtime_config.secrets.secret_key);
        let legacy = source(config());
        let mut stored = legacy.clone();
        stored.label = "updated".to_owned();
        stored.config =
            credentials::encrypt_config(&key, "example.com", Some("cloudflare"), &config())
                .unwrap();
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[legacy.clone()], [stored]])
            .into_connection();
        let db_log = db.clone();
        let state = ControlApiState::new(runtime_config, "unused.toml");
        state.database.set(db).unwrap();
        let request =
            serde_json::from_value(json!({"label": "updated", "config": {"api_token": ""}}))
                .unwrap();
        let response = update(State(state), Path(legacy.id), Json(request))
            .await
            .unwrap()
            .into_response();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-test-token"));
        let sql = format!("{:?}", db_log.into_transaction_log());
        assert!(sql.contains("ciphertext"));
        assert!(!sql.contains("private-test-token"));
    }
}
