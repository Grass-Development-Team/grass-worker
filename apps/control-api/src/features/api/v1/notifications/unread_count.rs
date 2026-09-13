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
        "/notifications/unread-count",
        axum::routing::get(unread_count),
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

async fn unread_count(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.unread_count";
    let count = notifications::unread_count(database(&state, OP)?, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(UnreadCountResponse { count }))
}

#[derive(serde::Serialize)]
struct UnreadCountResponse {
    count: u64,
}
