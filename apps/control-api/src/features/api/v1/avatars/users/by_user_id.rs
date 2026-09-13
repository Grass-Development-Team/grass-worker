pub(crate) mod by_version;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(by_version::router())
}
