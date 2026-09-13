pub(crate) mod authorize;
use crate::state::ControlApiState;
pub(crate) fn router(state: ControlApiState) -> axum::Router<ControlApiState> {
    authorize::router(state)
}
