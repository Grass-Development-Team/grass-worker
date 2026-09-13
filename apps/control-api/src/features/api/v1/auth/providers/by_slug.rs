pub(crate) mod callback;
pub(crate) mod start;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(callback::router())
        .merge(start::router())
}
