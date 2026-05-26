use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::{
    domain::users,
    infra::{
        error::{AppError, ok_response},
        http::session::Session,
    },
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "me.no_database",
        message: "database not available".to_owned(),
    })?;

    let user = users::get_user_by_id(db, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "me.get_user",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "me.user_not_found",
            message: "user not found".to_owned(),
        })?;

    Ok(ok_response(json!({
        "user": {
            "id": user.id,
            "email": user.email,
            "display_name": user.display_name,
        },
    })))
}
