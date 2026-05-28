use axum::{Router, middleware};

use crate::{infra::http::middlewares::csrf, state::ControlApiState};

pub mod v1;

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    let csrf_layer = middleware::from_fn_with_state(state.clone(), csrf::csrf_middleware);
    Router::new().nest("/v1", v1::router(state).layer(csrf_layer))
}
