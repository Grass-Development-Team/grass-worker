pub(crate) mod heartbeat;
pub(crate) mod register;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(heartbeat::router())
        .merge(register::router())
}
