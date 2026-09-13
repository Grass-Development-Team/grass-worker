pub(crate) mod migrations;
pub(crate) mod test;

use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::{storage_migrations, storage_settings},
    infra::{
        database::entity::storage_migration_job,
        error::{AppError, ok_response},
        storage::StorageConfig,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/storage", axum::routing::get(get))
        .merge(migrations::router())
        .merge(test::router())
}

/// GET /api/v1/admin/storage
async fn get(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.storage.get";
    let db = crate::infra::http::database(&state, OP)?;
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
    Ok(ok_response(GetResponse {
        storage: storage_view(&active.config, active.credentials.is_configured()),
        maintenance: state.storage.is_maintenance(),
        migration,
    }))
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
struct GetResponse {
    storage: StorageResponse,
    maintenance: bool,
    migration: Option<MigrationResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_config_never_contains_credentials() {
        let value = serde_json::to_value(storage_view(&StorageConfig::default(), true)).unwrap();
        assert_eq!(value["credentials_configured"], true);
        for field in ["access_key_id", "secret_access_key", "session_token"] {
            assert!(value.get(field).is_none());
        }
    }
}
