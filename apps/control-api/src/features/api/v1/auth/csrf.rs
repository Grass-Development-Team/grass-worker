use axum::{extract::State, response::IntoResponse};

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::{extractors::Session, middlewares::csrf},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/csrf", axum::routing::get(handler))
}

async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: "csrf.no_cache",
        message: "cache service not available".to_owned(),
    })?;

    let token = csrf::get_or_generate_csrf_token(cache, &session.session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "csrf.generate",
            source,
        })?;

    Ok(ok_response(ResponseBody { csrf_token: token }))
}

#[derive(serde::Serialize)]
struct ResponseBody {
    csrf_token: String,
}
