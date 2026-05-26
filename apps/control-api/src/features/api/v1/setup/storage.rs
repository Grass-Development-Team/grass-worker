use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct StorageSetupRequest {
    pub root: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<StorageSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let db = super::setup_database(&state, "setup.storage.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.storage.ready_mode").await?;

    let root = body.root.unwrap_or_else(|| "/data".to_owned());
    settings::set_string(db, "storage.root", &root)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.storage.save",
            source,
        })?;

    {
        let mut config = state.config.write().unwrap();
        config.storage.root = root.clone();
    }

    Ok(ok_response(json!({ "configured": true, "root": root })))
}
