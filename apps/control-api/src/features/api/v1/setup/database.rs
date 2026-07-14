use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    infra::{
        database,
        error::{AppError, ok_response},
    },
    init,
    state::ControlApiState,
};

use super::validate_postgres_url;

#[derive(Deserialize)]
pub struct DatabaseSetupRequest {
    pub url: String,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    Json(body): Json<DatabaseSetupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _setup_guard = state.lock_setup().await;
    if let Some(db) = state.try_database() {
        super::ensure_setup_mutation_allowed(db, "setup.database.ready_mode").await?;
        return Err(AppError::Conflict {
            op: "setup.database.already_configured",
            message: "database is already configured".to_owned(),
        });
    }

    validate_postgres_url(&body.url)?;
    let db = database::connect(&body.url)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.database.connect",
            source,
        })?;

    init::migrate_and_seed(&db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "setup.database.initialize",
            source,
        })?;

    {
        let mut config = state.config.write().unwrap();
        let mut persisted = crate::infra::config::ControlApiConfig::load_persisted(
            state.config_path(),
        )
        .map_err(|error| AppError::Infrastructure {
            op: "setup.database.load_config",
            source: anyhow::anyhow!(error),
        })?;
        persisted.database.url = body.url.clone();
        persisted.ensure_secret_key();
        persisted
            .save(state.config_path())
            .map_err(|error| AppError::Infrastructure {
                op: "setup.database.save_config",
                source: anyhow::anyhow!(error),
            })?;
        config.database.url = body.url;
    }

    state.database.set(db).map_err(|_| AppError::Internal {
        op: "setup.database.store_connection",
        message: "database connection already set".to_owned(),
    })?;

    Ok(ok_response(json!({
        "connected": true,
        "migrations_applied": true,
        "seed_completed": true,
    })))
}
