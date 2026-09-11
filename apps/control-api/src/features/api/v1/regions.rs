use crate::{
    domain::regions,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};
use axum::{extract::State, response::IntoResponse};

pub async fn list(
    State(state): State<ControlApiState>,
    _session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "regions.list";
    let db = super::admin::database(&state, OP)?;
    Ok(ok_response(regions::available(db).await.map_err(
        |source| AppError::Infrastructure { op: OP, source },
    )?))
}
