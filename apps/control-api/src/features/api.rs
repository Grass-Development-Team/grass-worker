use axum::{Router, middleware};

use crate::{infra::http::csrf, state::ControlApiState};

pub mod v1;

pub fn router(state: ControlApiState, is_setup_mode: bool) -> Router<ControlApiState> {
    let csrf_layer = middleware::from_fn_with_state(state, csrf::csrf_middleware);
    Router::new().nest("/v1", v1::router(is_setup_mode).layer(csrf_layer))
}
