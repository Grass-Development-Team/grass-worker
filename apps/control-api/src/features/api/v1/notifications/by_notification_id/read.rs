use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

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
        "/notifications/{notification_id}/read",
        axum::routing::post(mark_read),
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

pub async fn mark_read(
    State(state): State<ControlApiState>,
    session: Session,
    Path(notification_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "notifications.mark_read";
    let found =
        notifications::mark_read(database(&state, OP)?, session.data.user_id, notification_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if !found {
        return Err(AppError::NotFound {
            op: OP,
            message: "notification not found".to_owned(),
        });
    }
    Ok(ok_response(MarkReadResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct MarkReadResponse {
    ok: bool,
}
