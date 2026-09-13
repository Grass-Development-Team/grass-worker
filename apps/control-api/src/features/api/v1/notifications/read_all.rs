use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::notifications,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/notifications/read-all",
        axum::routing::post(mark_all_read),
    )
}

fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a sea_orm::DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}

pub async fn mark_all_read(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.mark_all_read";
    let updated = notifications::mark_all_read(database(&state, OP)?, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(MarkAllReadResponse { updated }))
}

#[derive(serde::Serialize)]
struct MarkAllReadResponse {
    updated: u64,
}
