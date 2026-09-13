use crate::state::ControlApiState;

pub(crate) mod authorize;
pub(crate) mod exchange;
pub(crate) mod verify;

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new()
        .merge(authorize::router())
        .merge(exchange::router())
        .merge(verify::router())
}
