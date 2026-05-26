use axum::Router;

use crate::state::ControlApiState;

pub mod v1;

pub fn router(is_setup_mode: bool) -> Router<ControlApiState> {
    Router::new().nest("/v1", v1::router(is_setup_mode))
}
