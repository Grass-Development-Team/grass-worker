use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;

use crate::{
    domain::storage_settings,
    infra::{
        error::{AppError, ok_response},
        storage::{StorageBackendKind, StorageConfig, StorageCredentials, build_backend},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/storage/test", axum::routing::post(test))
}

#[derive(Debug, Default, Deserialize)]
struct StorageRequest {
    backend: String,
    #[serde(flatten)]
    options: storage_settings::StorageOptions,
}

/// POST /api/v1/admin/storage/test
async fn test(
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
    Ok(ok_response(TestResponse { tested: true }))
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

#[derive(serde::Serialize)]
struct TestResponse {
    tested: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::state::ControlApiState;
    use serde_json::json;
    #[test]
    fn administration_defaults_to_the_current_root_and_requires_a_backend() {
        assert!(serde_json::from_value::<StorageRequest>(json!({})).is_err());
        let request: StorageRequest = serde_json::from_value(json!({"backend":"local"})).unwrap();
        let state =
            ControlApiState::new(crate::infra::config::ControlApiConfig::default(), "unused");
        let (config, _) = prepare(&state, request, "test.storage").unwrap();
        assert_eq!(config.local_root, state.storage.config().local_root);
    }
}
