pub(crate) mod start;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(start::router())
}
