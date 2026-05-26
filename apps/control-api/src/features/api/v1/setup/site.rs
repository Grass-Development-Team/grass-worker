use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::settings,
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct SiteSetupRequest {
    pub name: String,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<SiteSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let db = super::setup_database(&state, "setup.site.database")?;
    super::ensure_setup_mutation_allowed(db, "setup.site.ready_mode").await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation {
            op: "setup.site.empty_name",
            message: "site name cannot be empty".to_owned(),
        });
    }

    settings::set_string(db, "site.name", name)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.site.save",
            source,
        })?;

    Ok(ok_response(json!({ "configured": true, "name": name })))
}
