pub mod setup;

use axum::Router;

use crate::state::ControlApiState;

pub fn router(_is_setup_mode: bool) -> Router<ControlApiState> {
    Router::new().nest("/setup", setup::router())
}
