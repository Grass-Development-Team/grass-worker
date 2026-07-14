use axum::{extract::State, response::IntoResponse};
use serde::Serialize;

use crate::{
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

use super::{SetupStage, determine_stage};

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
