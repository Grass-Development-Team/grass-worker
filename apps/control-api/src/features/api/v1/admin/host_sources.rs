pub(crate) mod by_source_id;

use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::hosts::{self, CreateHostSourceParams, HostSourceError},
    infra::{
        database::entity::{HostSourceKind, host_source},
        error::{AppError, ok_response},
        host_provision::{self, cloudflare, credentials, dnspod, route53},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/host-sources", axum::routing::get(list).post(create))
        .merge(by_source_id::router())
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

fn parse_kind(value: &str, op: &'static str) -> Result<HostSourceKind, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "wildcard" => Ok(HostSourceKind::Wildcard),
        "dns_provider" => Ok(HostSourceKind::DnsProvider),
        "manual" => Ok(HostSourceKind::Manual),
        other => Err(AppError::Validation {
            op,
            message: format!("invalid host source kind: {other}"),
        }),
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

/// GET /api/v1/admin/host-sources
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.list";
    let db = crate::infra::http::database(&state, OP)?;
    let sources = hosts::list_sources(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(ListResponse {
        sources: sources.iter().map(source_view).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
pub struct CreateHostSourceRequest {
    pub kind: String,
    pub label: String,
    pub base_domain: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub allows_auto_assign: bool,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

const fn default_true() -> bool {
    true
}

fn default_region() -> String {
    "default".to_owned()
}

/// POST /api/v1/admin/host-sources
pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateHostSourceRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.create";
    let db = crate::infra::http::database(&state, OP)?;

    let kind = parse_kind(&body.kind, OP)?;
    if body.label.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "label is required".to_owned(),
        });
    }
    let base_domain = grass_validator::normalize_host(&body.base_domain).map_err(|error| {
        AppError::Validation {
            op: OP,
            message: format!("base_domain: {error}"),
        }
    })?;
    let region =
        grass_validator::normalize_region(&body.region).map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;

    crate::domain::regions::require(db, &region, OP).await?;
    let provider = body
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
        .map(str::to_ascii_lowercase);
    let config = merge_config(&json!({}), body.config.unwrap_or_else(|| json!({})), OP)?;
    if kind == HostSourceKind::DnsProvider {
        validate_dns_provider_source(provider.as_deref(), &base_domain, &config, OP)?;
    }
    let key = credentials::encryption_key(&state.config.read().unwrap().secrets.secret_key);
    let config = credentials::encrypt_config(&key, &base_domain, provider.as_deref(), &config)
        .map_err(|_| AppError::Internal {
            op: OP,
            message: "host source credentials could not be encrypted".to_owned(),
        })?;

    let source = hosts::create_source(
        db,
        CreateHostSourceParams {
            kind,
            label: body.label.trim().to_owned(),
            base_domain,
            region,
            enabled: body.enabled,
            allows_auto_assign: body.allows_auto_assign,
            is_default: body.is_default,
            provider,
            config,
        },
    )
    .await
    .map_err(|error| map_source_error(error, OP))?;

    Ok(ok_response(CreateResponse {
        source: source_view(&source),
    }))
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
struct ListResponse {
    sources: Vec<HostSourceResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    source: HostSourceResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use axum::body::to_bytes;
    use axum::extract::State;
    use axum::response::IntoResponse;
    use sea_orm::DbBackend;
    use sea_orm::MockDatabase;

    use crate::infra::database::entity::HostSourceKind;
    use crate::infra::database::entity::host_source;
    use crate::infra::host_provision::credentials;
    use crate::state::ControlApiState;
    use serde_json::json;
    use uuid::Uuid;
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

    fn config() -> serde_json::Value {
        json!({"api_token": "private-test-token", "zone_id": "test-zone", "record_type": "CNAME", "record_value": "entry.example.com"})
    }

    #[tokio::test]
    async fn create_writes_encrypted_config_and_returns_only_its_keys() {
        let mut runtime_config = crate::infra::config::ControlApiConfig::default();
        runtime_config.secrets.secret_key = "test-platform-key".to_owned();
        let key = credentials::encryption_key(&runtime_config.secrets.secret_key);
        let stored = source(
            credentials::encrypt_config(&key, "example.com", Some("cloudflare"), &config())
                .unwrap(),
        );
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[crate::infra::database::entity::region::Model {
                code: "default".into(),
                name: "Default".into(),
                created_at: time::OffsetDateTime::UNIX_EPOCH,
                updated_at: time::OffsetDateTime::UNIX_EPOCH,
            }]])
            .append_query_results([[stored]])
            .into_connection();
        let db_log = db.clone();
        let state = ControlApiState::new(runtime_config, "unused.toml");
        state.database.set(db).unwrap();
        let request = serde_json::from_value(json!({"kind": "dns_provider", "label": "test", "base_domain": "example.com", "provider": "cloudflare", "config": config()})).unwrap();
        let response = create(State(state), Json(request))
            .await
            .unwrap()
            .into_response();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(body["data"]["source"].get("config").is_none());
        assert!(
            body["data"]["source"]["config_keys"]
                .as_array()
                .unwrap()
                .contains(&json!("api_token"))
        );
        assert!(!String::from_utf8_lossy(&bytes).contains("private-test-token"));
        let sql = format!("{:?}", db_log.into_transaction_log());
        assert!(sql.contains("ciphertext"));
        assert!(!sql.contains("private-test-token"));
        assert!(!sql.contains("test-zone"));
    }
}
