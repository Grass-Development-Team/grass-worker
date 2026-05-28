pub mod auth;
pub mod me;
pub mod setup;

use axum::{Router, middleware, routing::get};

use crate::{
    infra::http::middlewares::setup_mode::{require_ready_mode, require_setup_mode},
    state::ControlApiState,
};

pub fn router(state: ControlApiState) -> Router<ControlApiState> {
    Router::new()
        .nest(
            "/setup",
            setup::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_setup_mode,
            )),
        )
        .nest(
            "/auth",
            auth::router().layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
        .route(
            "/me",
            get(me::handler).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ready_mode,
            )),
        )
}
