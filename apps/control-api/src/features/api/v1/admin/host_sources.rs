use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::hosts::{self, CreateHostSourceParams, HostSourceError, UpdateHostSourceParams},
    infra::{
        database::entity::{HostSourceKind, host_source},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

fn source_view(source: &host_source::Model) -> serde_json::Value {
    json!({
        "id": source.id,
        "kind": match source.kind {
            HostSourceKind::Wildcard => "wildcard",
            HostSourceKind::DnsProvider => "dns_provider",
            HostSourceKind::Manual => "manual",
        },
        "label": source.label,
        "base_domain": source.base_domain,
        "enabled": source.enabled,
        "allows_auto_assign": source.allows_auto_assign,
        "is_default": source.is_default,
        "provider": source.provider,
        // Config may carry provider credentials later; only expose keys.
        "config_keys": source
            .config
            .as_object()
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
        "created_at": ts(source.created_at),
    })
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

/// GET /api/v1/admin/host-sources
pub async fn list(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.list";
    let db = super::database(&state, OP)?;
    let sources = hosts::list_sources(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "sources": sources.iter().map(source_view).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct CreateHostSourceRequest {
    pub kind: String,
    pub label: String,
    pub base_domain: String,
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

/// POST /api/v1/admin/host-sources
pub async fn create(
    State(state): State<ControlApiState>,
    Json(body): Json<CreateHostSourceRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.create";
    let db = super::database(&state, OP)?;

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

    let source = hosts::create_source(
        db,
        CreateHostSourceParams {
            kind,
            label: body.label.trim().to_owned(),
            base_domain,
            enabled: body.enabled,
            allows_auto_assign: body.allows_auto_assign,
            is_default: body.is_default,
            provider: body.provider.filter(|provider| !provider.trim().is_empty()),
            config: body.config.unwrap_or_else(|| json!({})),
        },
    )
    .await
    .map_err(|error| map_source_error(error, OP))?;

    Ok(ok_response(json!({ "source": source_view(&source) })))
}

#[derive(Deserialize)]
pub struct UpdateHostSourceRequest {
    #[serde(default)]
    pub label: Option<String>,
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
    let db = super::database(&state, OP)?;

    let source = hosts::get_source_by_id(db, source_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host source not found".to_owned(),
        })?;

    let source = hosts::update_source(
        db,
        source,
        UpdateHostSourceParams {
            label: body.label.filter(|label| !label.trim().is_empty()),
            enabled: body.enabled,
            allows_auto_assign: body.allows_auto_assign,
            is_default: body.is_default,
            provider: body
                .provider
                .map(|provider| Some(provider).filter(|p| !p.trim().is_empty())),
            config: body.config,
        },
    )
    .await
    .map_err(|error| map_source_error(error, OP))?;

    Ok(ok_response(json!({ "source": source_view(&source) })))
}

/// DELETE /api/v1/admin/host-sources/{source_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Path(source_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.host_sources.remove";
    let db = super::database(&state, OP)?;

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

    Ok(ok_response(json!({ "ok": true })))
}
