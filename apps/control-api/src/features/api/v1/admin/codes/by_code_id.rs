pub(crate) mod revoke;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(revoke::router())
}
