pub mod auth;
pub mod me;
pub mod setup;

use axum::{Router, routing::get};

use crate::state::ControlApiState;

pub fn router(is_setup_mode: bool) -> Router<ControlApiState> {
    let mut router = Router::new();

    if is_setup_mode {
        router = router.nest("/setup", setup::router());
    } else {
        router = router
            .nest("/auth", auth::router())
            .route("/me", get(me::handler));
    }

    router
}
