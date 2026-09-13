pub(crate) mod by_domain_id;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(by_domain_id::router())
}
