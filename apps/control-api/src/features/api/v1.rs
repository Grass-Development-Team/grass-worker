pub mod setup;

use axum::Router;

use crate::state::ControlApiState;

pub fn router(is_setup_mode: bool) -> Router<ControlApiState> {
    let mut router = Router::new();
    if is_setup_mode {
        router = router.nest("/setup", setup::router());
    }
    router
}
