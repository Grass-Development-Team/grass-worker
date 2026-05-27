use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::{extractors::Session, middlewares::csrf},
    },
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let redis_conn = state.try_redis().ok_or_else(|| AppError::Internal {
        op: "csrf.no_redis",
        message: "session service not available".to_owned(),
    })?;

    let mut conn = redis_conn.clone();
    let token = csrf::generate_csrf_token(&mut conn, &session.session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "csrf.generate",
            source,
        })?;

    Ok(ok_response(json!({
        "csrf_token": token,
    })))
}
