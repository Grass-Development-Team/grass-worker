pub(crate) mod revoke;
pub(crate) mod rotate;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(revoke::router())
        .merge(rotate::router())
}
