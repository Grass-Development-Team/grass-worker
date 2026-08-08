use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{storage_migrations, storage_settings},
    infra::{
        database::entity::storage_migration_job,
        error::{AppError, accepted_response, ok_response},
        http::extractors::Session,
        storage::{StorageBackendKind, StorageConfig, StorageCredentials, build_backend},
    },
    state::ControlApiState,
};

#[derive(Debug, Default, Deserialize)]
pub struct StorageRequest {
    pub backend: String,
    #[serde(default)]
    pub local_root: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub force_path_style: Option<bool>,
    #[serde(default)]
    pub allow_http: Option<bool>,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    #[serde(default)]
    pub session_token: Option<String>,
}

/// GET /api/v1/admin/storage
pub async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.get";
    let db = super::database(&state, OP)?;
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let active =
        storage_settings::load_or_seed(db, &state.storage.config().local_root, &platform_secret)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let migration = storage_migrations::latest(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .map(public_job)
        .transpose()
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({
        "storage": storage_settings::public_config(
            &active.config,
            active.credentials.is_configured(),
        ),
        "maintenance": state.storage.is_maintenance(),
        "migration": migration,
    })))
}

/// POST /api/v1/admin/storage/test
pub async fn test(
    State(state): State<ControlApiState>,
    Json(body): Json<StorageRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.test";
    let (config, credentials) = prepare(&state, body, OP)?;
    let backend = build_backend(&config, &credentials).map_err(|source| AppError::Validation {
        op: OP,
        message: source.to_string(),
    })?;
    backend
        .probe()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(json!({ "tested": true })))
}

/// POST /api/v1/admin/storage/migrations
pub async fn create_migration(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<StorageRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.migrations.create";
    let db = super::database(&state, OP)?;
    if storage_migrations::has_active(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        return Err(AppError::Conflict {
            op: OP,
            message: "a storage migration is already active".to_owned(),
        });
    }
    let (config, credentials) = prepare(&state, body, OP)?;
    let backend = build_backend(&config, &credentials).map_err(|source| AppError::Validation {
        op: OP,
        message: source.to_string(),
    })?;
    backend
        .probe()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let job = storage_migrations::create(&state, session.data.user_id, config, credentials)
        .await
        .map_err(|source| AppError::Validation {
            op: OP,
            message: source.to_string(),
        })?;
    Ok(accepted_response(
        json!({ "migration": public_job(job).map_err(|source| {
        AppError::Infrastructure { op: OP, source }
    })? }),
    ))
}

/// GET /api/v1/admin/storage/migrations
pub async fn migration(
    State(state): State<ControlApiState>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.migrations.get";
    let db = super::database(&state, OP)?;
    let migration = storage_migrations::latest(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .map(public_job)
        .transpose()
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({
        "maintenance": state.storage.is_maintenance(),
        "migration": migration,
    })))
}

fn prepare(
    state: &ControlApiState,
    body: StorageRequest,
    op: &'static str,
) -> Result<(StorageConfig, StorageCredentials), AppError> {
    let backend = body
        .backend
        .parse::<StorageBackendKind>()
        .map_err(|source| AppError::Validation {
            op,
            message: source.to_string(),
        })?;
    let current = state.storage.config();
    let config = StorageConfig {
        backend,
        local_root: body
            .local_root
            .unwrap_or(current.local_root)
            .trim()
            .trim_end_matches('/')
            .to_owned(),
        endpoint: body.endpoint.unwrap_or_default().trim().to_owned(),
        region: body
            .region
            .unwrap_or_else(|| backend.default_region().to_owned())
            .trim()
            .to_owned(),
        bucket: body.bucket.unwrap_or_default().trim().to_owned(),
        prefix: body.prefix.unwrap_or_default().trim_matches('/').to_owned(),
        force_path_style: body
            .force_path_style
            .unwrap_or(matches!(backend, StorageBackendKind::Minio)),
        allow_http: body
            .allow_http
            .unwrap_or(matches!(backend, StorageBackendKind::Minio)),
    };
    config.validate().map_err(|source| AppError::Validation {
        op,
        message: source.to_string(),
    })?;
    let credentials = StorageCredentials {
        access_key_id: clean_secret(body.access_key_id),
        secret_access_key: clean_secret(body.secret_access_key),
        session_token: clean_secret(body.session_token),
    };
    Ok((config, credentials))
}

fn clean_secret(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn public_job(job: storage_migration_job::Model) -> anyhow::Result<serde_json::Value> {
    let source: StorageConfig = serde_json::from_value(job.source_config)?;
    let target: StorageConfig = serde_json::from_value(job.target_config)?;
    Ok(json!({
        "id": job.id,
        "status": storage_migrations::status_value(&job.status),
        "source": storage_settings::public_config(&source, job.source_credentials.is_some()),
        "target": storage_settings::public_config(&target, job.target_credentials.is_some()),
        "copied_objects": job.copied_objects,
        "copied_bytes": job.copied_bytes,
        "total_objects": job.total_objects,
        "total_bytes": job.total_bytes,
        "last_error": job.last_error,
        "created_at": job.created_at.unix_timestamp(),
        "started_at": job.started_at.map(|value| value.unix_timestamp()),
        "finished_at": job.finished_at.map(|value| value.unix_timestamp()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_trimmed_without_being_exposed() {
        assert_eq!(
            clean_secret(Some(" secret ".to_owned())).as_deref(),
            Some("secret")
        );
        assert_eq!(clean_secret(Some("  ".to_owned())), None);
    }
}
