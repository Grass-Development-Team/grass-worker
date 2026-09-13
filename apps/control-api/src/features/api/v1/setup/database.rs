use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use url::Url;

use crate::{
    infra::{
        database,
        error::{AppError, ok_response},
    },
    init,
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/database", axum::routing::post(handler))
}

pub(crate) async fn ensure_setup_mutation_allowed(
    db: &DatabaseConnection,
    op: &'static str,
) -> Result<(), AppError> {
    if init::is_setup_finished(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::SetupNotAllowed {
            op,
            message: "setup has already finished".to_owned(),
        });
    }
    Ok(())
}

pub(crate) fn validate_postgres_url(url: &str) -> Result<(), AppError> {
    let parsed = Url::parse(url).map_err(|error| AppError::Validation {
        op: "setup.database.invalid_url",
        message: format!("invalid database URL: {error}"),
    })?;

    match parsed.scheme() {
        "postgres" | "postgresql" => Ok(()),
        scheme => Err(AppError::Validation {
            op: "setup.database.unsupported_scheme",
            message: format!("unsupported database scheme: {scheme}"),
        }),
    }
}

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
        ensure_setup_mutation_allowed(db, "setup.database.ready_mode").await?;
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

    Ok(ok_response(ResponseBody {
        connected: true,
        migrations_applied: true,
        seed_completed: true,
    }))
}

#[derive(serde::Serialize)]
struct ResponseBody {
    connected: bool,
    migrations_applied: bool,
    seed_completed: bool,
}
