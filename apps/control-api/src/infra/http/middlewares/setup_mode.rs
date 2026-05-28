use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{init, state::ControlApiState};

pub async fn require_setup_mode(
    State(state): State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let finished = match state.try_database() {
        Some(db) => init::is_setup_finished(db).await.unwrap_or(false),
        None => false,
    };
    if finished {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}

pub async fn require_ready_mode(
    State(state): State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let finished = match state.try_database() {
        Some(db) => init::is_setup_finished(db).await.unwrap_or(false),
        None => false,
    };
    if !finished {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(request).await
}
