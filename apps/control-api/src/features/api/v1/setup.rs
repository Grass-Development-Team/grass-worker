use axum::{
    Router,
    routing::{get, post},
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    domain::{nodes, settings, users},
    infra::error::AppError,
    init,
    state::ControlApiState,
};

pub mod admin;
pub mod database;
pub mod finish;
pub mod node;
pub mod site;
pub mod state;
pub mod storage;

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/state", get(state::handler))
        .route("/database", post(database::handler))
        .route("/admin", post(admin::handler))
        .route("/site", post(site::handler))
        .route("/node", post(node::handler))
        .route("/storage", post(storage::handler))
        .route("/finish", post(finish::handler))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    Database,
    Admin,
    Site,
    Node,
    Storage,
    Finish,
    Complete,
}

pub(crate) async fn determine_stage(state: &ControlApiState) -> Result<SetupStage, AppError> {
    let Some(db) = state.try_database() else {
        return Ok(SetupStage::Database);
    };

    if init::is_setup_finished(db)
        .await
        .map_err(|source| setup_state_error("setup.state.finished", source))?
    {
        return Ok(SetupStage::Complete);
    }

    if !users::any_user_exists(db)
        .await
        .map_err(|source| setup_state_error("setup.state.admin", source))?
    {
        return Ok(SetupStage::Admin);
    }

    if settings::get_setting(db, "site.name")
        .await
        .map_err(|source| setup_state_error("setup.state.site", source))?
        .is_none()
    {
        return Ok(SetupStage::Site);
    }

    if !nodes::any_node_exists(db)
        .await
        .map_err(|source| setup_state_error("setup.state.node", source))?
    {
        return Ok(SetupStage::Node);
    }

    if settings::get_setting(db, "storage.root")
        .await
        .map_err(|source| setup_state_error("setup.state.storage", source))?
        .is_none()
    {
        return Ok(SetupStage::Storage);
    }

    Ok(SetupStage::Finish)
}

fn setup_state_error(op: &'static str, source: anyhow::Error) -> AppError {
    AppError::Infrastructure { op, source }
}

pub(crate) fn setup_database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Validation {
        op,
        message: "database must be configured first".to_owned(),
    })
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
