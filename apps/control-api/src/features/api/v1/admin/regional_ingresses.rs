use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::ingress,
    infra::{
        database::{self, entity::regional_ingress},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

fn ingress_view(item: &regional_ingress::Model) -> serde_json::Value {
    json!({
        "id": item.id,
        "region": item.region,
        "hostname": item.hostname,
        "enabled": item.enabled,
        "health_check_path": item.health_check_path,
        "health_check_interval_seconds": item.health_check_interval_seconds,
        "origin_host_preservation": item.origin_host_preservation,
        "tls_enabled": item.tls_enabled,
        "certificate_issuer": item.certificate_issuer,
        "certificate_auto_renew": item.certificate_auto_renew,
        "certificate_status": item.certificate_status,
        "certificate_expires_at": ts(item.certificate_expires_at),
        "certificate_error": item.certificate_error,
        "dns_challenge_provider": item.dns_challenge_provider,
        "dns_challenge_config_keys": item.dns_challenge_config.as_object().map(|object| object.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),
        "dns_challenge_status": item.dns_challenge_status,
        "dns_challenge_record_name": item.dns_challenge_record_name,
        "dns_challenge_record_value": item.dns_challenge_record_value,
        "healthy_nodes": [],
        "deleted_at": ts(item.deleted_at),
        "created_at": ts(item.created_at),
        "updated_at": ts(item.updated_at),
    })
}

fn parse_issuer(value: &str, op: &'static str) -> Result<String, AppError> {
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "letsencrypt" | "zerossl" | "manual") {
        Ok(normalized)
    } else {
        Err(AppError::Validation {
            op,
            message: "certificate_issuer must be letsencrypt, zerossl, or manual".to_owned(),
        })
    }
}

fn validate_challenge(
    provider: Option<&str>,
    config: &serde_json::Value,
    op: &'static str,
) -> Result<Option<String>, AppError> {
    let provider = provider
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    if provider.is_some() && !config.is_object() {
        return Err(AppError::Validation {
            op,
            message: "dns_challenge_config must be a JSON object".to_owned(),
        });
    }
    Ok(provider)
}

fn validate_health_check(path: &str, interval: i32, op: &'static str) -> Result<String, AppError> {
    let path = path.trim();
    if !path.starts_with('/') || path.len() > 512 {
        return Err(AppError::Validation {
            op,
            message: "health_check_path must start with / and be at most 512 bytes".to_owned(),
        });
    }
    if !(5..=3600).contains(&interval) {
        return Err(AppError::Validation {
            op,
            message: "health_check_interval_seconds must be between 5 and 3600".to_owned(),
        });
    }
    Ok(path.to_owned())
}

#[derive(Deserialize)]
pub struct CreateRegionalIngressRequest {
    pub region: String,
    pub hostname: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_health_path")]
    pub health_check_path: String,
    #[serde(default = "default_health_interval")]
    pub health_check_interval_seconds: i32,
    #[serde(default = "default_true")]
    pub origin_host_preservation: bool,
    #[serde(default = "default_true")]
    pub tls_enabled: bool,
    #[serde(default = "default_issuer")]
    pub certificate_issuer: String,
    #[serde(default = "default_true")]
    pub certificate_auto_renew: bool,
    #[serde(default)]
    pub dns_challenge_provider: Option<String>,
    #[serde(default)]
    pub dns_challenge_config: serde_json::Value,
}

const fn default_true() -> bool {
    true
}

fn default_health_path() -> String {
    "/health".to_owned()
}

const fn default_health_interval() -> i32 {
    30
}

fn default_issuer() -> String {
    "letsencrypt".to_owned()
}

/// GET /api/v1/admin/regional-ingresses
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.list";
    let db = super::database(&state, OP)?;
    let items = ingress::list(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let now = OffsetDateTime::now_utc();
    let mut views = Vec::with_capacity(items.len());
    for item in &items {
        let candidates = ingress::healthy_serve_nodes(db, &item.region, now)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let mut view = ingress_view(item);
        view["healthy_nodes"] = json!(
            candidates
                .iter()
                .map(|candidate| json!({
                    "node_id": candidate.node_id,
                    "base_url": candidate.base_url,
                    "priority": candidate.priority,
                }))
                .collect::<Vec<_>>()
        );
        views.push(view);
    }
    Ok(ok_response(json!({ "regional_ingresses": views })))
}

/// POST /api/v1/admin/regional-ingresses
pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateRegionalIngressRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.create";
    let db = super::database(&state, OP)?;
    let region =
        grass_validator::normalize_region(&body.region).map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;
    let hostname =
        grass_validator::normalize_host(&body.hostname).map_err(|error| AppError::Validation {
            op: OP,
            message: format!("hostname: {error}"),
        })?;
    let health_check_path = validate_health_check(
        &body.health_check_path,
        body.health_check_interval_seconds,
        OP,
    )?;
    let certificate_issuer = parse_issuer(&body.certificate_issuer, OP)?;
    let dns_challenge_config = if body.dns_challenge_config.is_null() {
        json!({})
    } else {
        body.dns_challenge_config
    };
    let dns_challenge_provider = validate_challenge(
        body.dns_challenge_provider.as_deref(),
        &dns_challenge_config,
        OP,
    )?;
    let now = OffsetDateTime::now_utc();
    let item = regional_ingress::ActiveModel {
        id: Set(Uuid::now_v7()),
        region: Set(region),
        hostname: Set(hostname),
        enabled: Set(body.enabled),
        health_check_path: Set(health_check_path),
        health_check_interval_seconds: Set(body.health_check_interval_seconds),
        origin_host_preservation: Set(body.origin_host_preservation),
        tls_enabled: Set(body.tls_enabled),
        certificate_issuer: Set(certificate_issuer),
        certificate_auto_renew: Set(body.certificate_auto_renew),
        certificate_status: Set(if body.tls_enabled {
            "pending"
        } else {
            "disabled"
        }
        .to_owned()),
        certificate_expires_at: Set(None),
        certificate_error: Set(None),
        dns_challenge_provider: Set(dns_challenge_provider.clone()),
        dns_challenge_config: Set(dns_challenge_config),
        dns_challenge_status: Set(if dns_challenge_provider.is_some() {
            "pending"
        } else {
            "not_configured"
        }
        .to_owned()),
        dns_challenge_record_name: Set(None),
        dns_challenge_record_value: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(|source| {
        let source: anyhow::Error = source.into();
        if database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "an active regional ingress already uses this region or hostname"
                    .to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    Ok(ok_response(
        json!({ "regional_ingress": ingress_view(&item) }),
    ))
}

#[derive(Deserialize)]
pub struct UpdateRegionalIngressRequest {
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub health_check_path: Option<String>,
    #[serde(default)]
    pub health_check_interval_seconds: Option<i32>,
    #[serde(default)]
    pub origin_host_preservation: Option<bool>,
    #[serde(default)]
    pub tls_enabled: Option<bool>,
    #[serde(default)]
    pub certificate_issuer: Option<String>,
    #[serde(default)]
    pub certificate_auto_renew: Option<bool>,
    #[serde(default)]
    pub dns_challenge_provider: Option<String>,
    #[serde(default)]
    pub dns_challenge_config: Option<serde_json::Value>,
}

/// PATCH /api/v1/admin/regional-ingresses/{ingress_id}
pub async fn update(
    State(state): State<ControlApiState>,
    Path(ingress_id): Path<Uuid>,
    Json(body): Json<UpdateRegionalIngressRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.update";
    let db = super::database(&state, OP)?;
    let item = ingress::get_by_id(db, ingress_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress not found".to_owned(),
        })?;
    let mut active: regional_ingress::ActiveModel = item.clone().into();
    if let Some(hostname) = body.hostname {
        active.hostname = Set(grass_validator::normalize_host(&hostname).map_err(|error| {
            AppError::Validation {
                op: OP,
                message: format!("hostname: {error}"),
            }
        })?);
    }
    let has_health_check_path = body.health_check_path.is_some();
    let has_health_check_interval = body.health_check_interval_seconds.is_some();
    let next_interval = body
        .health_check_interval_seconds
        .unwrap_or(item.health_check_interval_seconds);
    if let Some(path) = body.health_check_path.as_deref() {
        active.health_check_path = Set(validate_health_check(path, next_interval, OP)?);
    } else if has_health_check_interval {
        validate_health_check(&item.health_check_path, next_interval, OP)?;
        active.health_check_interval_seconds = Set(next_interval);
    }
    if has_health_check_path && has_health_check_interval {
        active.health_check_interval_seconds = Set(next_interval);
    }
    if let Some(value) = body.enabled {
        active.enabled = Set(value);
        active.certificate_status = Set(if value && item.tls_enabled {
            "pending"
        } else {
            "disabled"
        }
        .to_owned());
    }
    if let Some(value) = body.origin_host_preservation {
        active.origin_host_preservation = Set(value);
    }
    if let Some(value) = body.tls_enabled {
        active.tls_enabled = Set(value);
        active.certificate_status = Set(if value && item.enabled {
            "pending"
        } else {
            "disabled"
        }
        .to_owned());
    }
    if let Some(value) = body.certificate_issuer {
        active.certificate_issuer = Set(parse_issuer(&value, OP)?);
    }
    if let Some(value) = body.certificate_auto_renew {
        active.certificate_auto_renew = Set(value);
    }
    let provider = body.dns_challenge_provider.map(Some);
    let config = body.dns_challenge_config;
    if provider.is_some() || config.is_some() {
        let provider_value = provider
            .flatten()
            .or_else(|| item.dns_challenge_provider.clone());
        let config_value = config.unwrap_or_else(|| item.dns_challenge_config.clone());
        let provider_value = validate_challenge(provider_value.as_deref(), &config_value, OP)?;
        active.dns_challenge_provider = Set(provider_value.clone());
        active.dns_challenge_config = Set(config_value);
        active.dns_challenge_status = Set(if provider_value.is_some() {
            "pending"
        } else {
            "not_configured"
        }
        .to_owned());
    }
    active.updated_at = Set(OffsetDateTime::now_utc());
    let item = active.update(db).await.map_err(|source| {
        let source: anyhow::Error = source.into();
        if database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "an active regional ingress already uses this hostname".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    Ok(ok_response(
        json!({ "regional_ingress": ingress_view(&item) }),
    ))
}

/// DELETE /api/v1/admin/regional-ingresses/{ingress_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Path(ingress_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.remove";
    let db = super::database(&state, OP)?;
    let item = ingress::get_by_id(db, ingress_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress not found".to_owned(),
        })?;
    let mut active: regional_ingress::ActiveModel = item.into();
    active.enabled = Set(false);
    active.certificate_status = Set("disabled".to_owned());
    active.deleted_at = Set(Some(OffsetDateTime::now_utc()));
    active.updated_at = Set(OffsetDateTime::now_utc());
    active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::{parse_issuer, validate_health_check};

    #[test]
    fn regional_ingress_validation_accepts_supported_controls() {
        assert_eq!(parse_issuer(" ZeroSSL ", "test").unwrap(), "zerossl");
        assert_eq!(
            validate_health_check("/ready", 30, "test").unwrap(),
            "/ready"
        );
    }

    #[test]
    fn regional_ingress_validation_rejects_unsafe_controls() {
        assert!(parse_issuer("acme", "test").is_err());
        assert!(validate_health_check("ready", 30, "test").is_err());
        assert!(validate_health_check("/ready", 4, "test").is_err());
    }
}
