use axum::{extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{nodes, settings, users},
    infra::error::{AppError, ok_response},
    init,
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/state", axum::routing::get(handler))
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

    if settings::get_setting(db, crate::domain::storage_settings::CONFIG_KEY)
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

#[derive(Serialize)]
pub struct SetupStateResponse {
    pub stage: SetupStage,
    pub is_setup_mode: bool,
}

pub async fn handler(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    let stage = determine_stage(&state).await?;
    let is_setup_mode = stage != SetupStage::Complete;

    Ok(ok_response(SetupStateResponse {
        stage,
        is_setup_mode,
    }))
}
