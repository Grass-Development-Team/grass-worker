use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{certificates, ingress},
    infra::{
        database::{
            self,
            entity::{
                managed_certificate, node, node_ingress_status, regional_ingress,
                regional_ingress_health,
            },
        },
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
        "certificate_issued_at": ts(item.certificate_issued_at),
        "certificate_error": item.certificate_error,
        "dns_challenge_provider": item.dns_challenge_provider,
        "dns_challenge_config_keys": [],
        "dns_challenge_status": item.dns_challenge_status,
        "dns_challenge_record_name": item.dns_challenge_record_name,
        "dns_challenge_record_value": item.dns_challenge_record_value,
        "healthy_nodes": [],
        "deleted_at": ts(item.deleted_at),
        "created_at": ts(item.created_at),
        "updated_at": ts(item.updated_at),
    })
}

async fn detailed_view(
    db: &sea_orm::DatabaseConnection,
    item: &regional_ingress::Model,
    secret: &str,
) -> anyhow::Result<serde_json::Value> {
    let mut view = ingress_view(item);
    view["dns_challenge_config_keys"] = json!(
        certificates::config(item, secret)?
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    );
    let cert = managed_certificate::Entity::find_by_id(item.id)
        .one(db)
        .await?;
    view["certificate_revision"] = json!(
        cert.as_ref()
            .map(|c| c.revision.as_str())
            .unwrap_or_default()
    );
    view["certificate_retry_at"] = ts(cert.as_ref().and_then(|c| c.retry_at));
    if let Some(cert) = &cert {
        view["certificate_status"] = json!(cert.status);
        view["certificate_error"] = json!(cert.error);
        view["certificate_expires_at"] = ts(cert.expires_at);
        view["certificate_issued_at"] = ts(cert.issued_at);
    }
    let nodes = node::Entity::find()
        .filter(node::Column::Region.eq(&item.region))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    let mut statuses = Vec::new();
    for node in nodes {
        let status = node_ingress_status::Entity::find_by_id(node.id)
            .one(db)
            .await?;
        let health = regional_ingress_health::Entity::find_by_id((item.id, node.id))
            .one(db)
            .await?;
        let installed = status
            .as_ref()
            .and_then(|s| s.certificates.as_array())
            .and_then(|rows| {
                rows.iter()
                    .find(|c| c["ingress_id"].as_str() == Some(&item.id.to_string()))
            })
            .and_then(|c| c["revision"].as_str());
        statuses.push(json!({"node_id":node.id,"tls_ready":status.as_ref().is_some_and(|s|s.tls_ready),"challenge_revision":status.as_ref().map(|s|s.challenge_revision.as_str()).unwrap_or_default(),"checked_at":ts(status.as_ref().map(|s|s.checked_at)),"certificate_revision":installed,"health_status":health.as_ref().map(|h|h.status.as_str()).unwrap_or("unknown"),"health_checked_at":ts(health.as_ref().and_then(|h|h.checked_at)),"health_error":health.as_ref().and_then(|h|h.error.as_deref())}));
    }
    view["node_statuses"] = json!(statuses);
    if !item.enabled || !item.tls_enabled {
        view["certificate_status"] = json!("disabled");
    }
    Ok(view)
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
    if provider
        .as_ref()
        .is_some_and(|p| !matches!(p.as_str(), "cloudflare" | "dnspod" | "route53"))
    {
        return Err(AppError::Validation {
            op,
            message: "unsupported DNS challenge provider".to_owned(),
        });
    }
    if provider.is_some() && !config.is_object() {
        return Err(AppError::Validation {
            op,
            message: "dns_challenge_config must be a JSON object".to_owned(),
        });
    }
    let required: &[&str] = match provider.as_deref() {
        Some("cloudflare") => &["api_token", "zone_id"],
        Some("dnspod") => &["secret_id", "secret_key", "domain"],
        Some("route53") => &["access_key_id", "secret_access_key", "hosted_zone_id"],
        _ => &[],
    };
    for field in required {
        if config
            .get(*field)
            .and_then(serde_json::Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(AppError::Validation {
                op,
                message: format!("dns_challenge_config.{field} is required"),
            });
        }
    }
    Ok(provider)
}

fn validate_health_check(path: &str, interval: i32, op: &'static str) -> Result<String, AppError> {
    let path = path.trim();
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains(['?', '#', '\\'])
        || path.len() > 512
    {
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
    "/_grass/health".to_owned()
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
        let secret = state.config.read().unwrap().secrets.secret_key.clone();
        let mut view = detailed_view(db, item, &secret)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
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
    let id = Uuid::now_v7();
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let dns_challenge_config = certificates::seal_config(id, &dns_challenge_config, &secret)
        .map_err(|source| AppError::Validation {
            op: OP,
            message: source.to_string(),
        })?;
    let item = regional_ingress::ActiveModel {
        id: Set(id),
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
        acme_account: Set(None),
        certificate_bundle: Set(None),
        certificate_issued_at: Set(None),
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
        json!({ "regional_ingress": detailed_view(db,&item,&secret).await.map_err(|source| AppError::Infrastructure {op:OP,source})? }),
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
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let item = regional_ingress::Entity::find_by_id(ingress_id)
        .filter(regional_ingress::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress not found".to_owned(),
        })?;
    let mut active: regional_ingress::ActiveModel = item.clone().into();
    let old_issuer = item.certificate_issuer.clone();
    let configuration_changed =
        body.dns_challenge_config.is_some() || body.dns_challenge_provider.is_some();
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let records = managed_certificate::Entity::find()
        .filter(managed_certificate::Column::IngressId.eq(item.id))
        .lock_exclusive()
        .all(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if (body.hostname.is_some()
        || body.certificate_issuer.is_some()
        || body.dns_challenge_config.is_some()
        || body.dns_challenge_provider.is_some())
        && records.iter().any(|record| {
            record
                .lease_until
                .is_some_and(|until| until > OffsetDateTime::now_utc())
        })
    {
        return Err(AppError::Conflict{op:OP,message:"wait for certificate issuance to finish before changing hostname, issuer or DNS credentials".to_owned()});
    }
    let mut reset_certificate = false;
    if let Some(hostname) = body.hostname {
        reset_certificate = hostname.trim() != item.hostname;
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
        let issuer = parse_issuer(&value, OP)?;
        reset_certificate |= issuer != item.certificate_issuer;
        active.certificate_issuer = Set(issuer);
        if reset_certificate {
            active.acme_account = Set(None);
        }
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
        let previous = certificates::config(&item, &secret)
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let config_value = match config {
            Some(patch) => certificates::merge_config(previous, &patch).map_err(|source| {
                AppError::Validation {
                    op: OP,
                    message: source.to_string(),
                }
            })?,
            None => previous,
        };
        let provider_value = validate_challenge(provider_value.as_deref(), &config_value, OP)?;
        active.dns_challenge_provider = Set(provider_value.clone());
        active.dns_challenge_config = Set(certificates::seal_config(
            item.id,
            &config_value,
            &secret,
        )
        .map_err(|source| AppError::Validation {
            op: OP,
            message: source.to_string(),
        })?);
        active.dns_challenge_status = Set(if provider_value.is_some() {
            "pending"
        } else {
            "not_configured"
        }
        .to_owned());
    }
    active.updated_at = Set(OffsetDateTime::now_utc());
    let item = active.update(&transaction).await.map_err(|source| {
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
    if reset_certificate || configuration_changed {
        for record in records {
            let mut cert: managed_certificate::ActiveModel = record.clone().into();
            cert.generation = Set(Uuid::now_v7());
            let reissue = reset_certificate
                && (record.host_binding_id.is_none()
                    || (record.issuer != "manual" && old_issuer != item.certificate_issuer));
            if !reissue {
                cert.update(&transaction)
                    .await
                    .map_err(|source| AppError::Infrastructure {
                        op: OP,
                        source: source.into(),
                    })?;
                continue;
            }
            cert.lease_until = Set(None);
            cert.status = Set("pending".to_owned());
            cert.retry_at = Set(None);
            cert.failure_count = Set(0);
            cert.acme_account = Set(None);
            cert.issuer = Set(item.certificate_issuer.clone());
            cert.challenge_token = Set(None);
            cert.challenge_value = Set(None);
            cert.challenge_expires_at = Set(None);
            if record.host_binding_id.is_none() && record.hostname != item.hostname {
                cert.hostname = Set(item.hostname.clone());
                cert.bundle = Set(None);
                cert.revision = Set(String::new());
                cert.issued_at = Set(None);
                cert.expires_at = Set(None);
            }
            cert.update(&transaction)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(
        json!({ "regional_ingress": detailed_view(db,&item,&secret).await.map_err(|source|AppError::Infrastructure{op:OP,source})? }),
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

/// POST /api/v1/admin/regional-ingresses/{ingress_id}/certificate/renew
pub async fn renew_certificate(
    State(state): State<ControlApiState>,
    Path(ingress_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.renew_certificate";
    let db = super::database(&state, OP)?;
    let item = ingress::get_by_id(db, ingress_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress not found".to_owned(),
        })?;
    if !item.enabled || !item.tls_enabled {
        return Err(AppError::Conflict {
            op: OP,
            message: "enable this ingress and TLS before renewing".to_owned(),
        });
    }
    let cert = certificates::ensure_record(db, &item, None)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    certificates::queue(db, &cert)
        .await
        .map_err(|source| AppError::Conflict {
            op: OP,
            message: source.to_string(),
        })?;
    let refreshed = ingress::get_by_id(db, ingress_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress was removed during renewal".to_owned(),
        })?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    Ok(ok_response(
        json!({ "regional_ingress": detailed_view(db,&refreshed,&secret).await.map_err(|source|AppError::Infrastructure{op:OP,source})? }),
    ))
}

pub async fn import_certificate(
    State(state): State<ControlApiState>,
    Path(id): Path<Uuid>,
    Json(body): Json<certificates::PemBundle>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.regional_ingresses.import_certificate";
    let db = super::database(&state, OP)?;
    let item = ingress::get_by_id(db, id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "regional ingress not found".to_owned(),
        })?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let cert = certificates::ensure_record(db, &item, None)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    certificates::import(db, &cert, body, &secret)
        .await
        .map_err(|source| AppError::Validation {
            op: OP,
            message: source.to_string(),
        })?;
    let mut active: regional_ingress::ActiveModel = item.into();
    active.certificate_issuer = Set("manual".to_owned());
    active.certificate_auto_renew = Set(false);
    let item = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(
        json!({"regional_ingress":detailed_view(db,&item,&secret).await.map_err(|source|AppError::Infrastructure{op:OP,source})?}),
    ))
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
