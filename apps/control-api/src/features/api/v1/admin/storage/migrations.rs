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

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/storage/migrations",
        axum::routing::get(migration).post(create_migration),
    )
}

#[derive(Debug, Default, Deserialize)]
pub struct StorageRequest {
    pub backend: String,
    #[serde(flatten)]
    pub options: storage_settings::StorageOptions,
}

/// POST /api/v1/admin/storage/migrations
pub async fn create_migration(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<StorageRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.migrations.create";
    let db = crate::infra::http::database(&state, OP)?;
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
    let db = crate::infra::http::database(&state, OP)?;
    let migration = storage_migrations::latest(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .map(public_job)
        .transpose()
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(MigrationStatusResponse {
        maintenance: state.storage.is_maintenance(),
        migration,
    }))
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
    body.options
        .resolve(backend, &state.storage.config().local_root)
        .map_err(|source| AppError::Validation {
            op,
            message: source.to_string(),
        })
}

fn public_job(job: storage_migration_job::Model) -> anyhow::Result<MigrationResponse> {
    let source: StorageConfig = serde_json::from_value(job.source_config)?;
    let target: StorageConfig = serde_json::from_value(job.target_config)?;
    Ok(MigrationResponse {
        id: job.id,
        status: storage_migrations::status_value(&job.status),
        source: storage_view(&source, job.source_credentials.is_some()),
        target: storage_view(&target, job.target_credentials.is_some()),
        copied_objects: job.copied_objects,
        copied_bytes: job.copied_bytes,
        total_objects: job.total_objects,
        total_bytes: job.total_bytes,
        last_error: job.last_error,
        created_at: job.created_at.unix_timestamp(),
        started_at: job.started_at.map(|value| value.unix_timestamp()),
        finished_at: job.finished_at.map(|value| value.unix_timestamp()),
    })
}

#[derive(serde::Serialize)]
struct MigrationResponse {
    id: uuid::Uuid,
    status: &'static str,
    source: StorageResponse,
    target: StorageResponse,
    copied_objects: i64,
    copied_bytes: i64,
    total_objects: Option<i64>,
    total_bytes: Option<i64>,
    last_error: Option<String>,
    created_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

#[derive(serde::Serialize)]
struct StorageResponse {
    backend: &'static str,
    local_root: String,
    endpoint: String,
    region: String,
    bucket: String,
    prefix: String,
    force_path_style: bool,
    allow_http: bool,
    credentials_configured: bool,
}
fn storage_view(
    config: &crate::infra::storage::StorageConfig,
    credentials_configured: bool,
) -> StorageResponse {
    StorageResponse {
        backend: config.backend.as_str(),
        local_root: config.local_root.clone(),
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        bucket: config.bucket.clone(),
        prefix: config.prefix.clone(),
        force_path_style: config.force_path_style,
        allow_http: config.allow_http,
        credentials_configured,
    }
}

#[derive(serde::Serialize)]
struct MigrationStatusResponse {
    maintenance: bool,
    migration: Option<MigrationResponse>,
}
