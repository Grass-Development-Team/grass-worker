pub(crate) mod assignments;
pub(crate) mod certificates;
pub(crate) mod deployments;
pub(crate) mod ingress_status;
pub(crate) mod preview;
pub(crate) mod resolve_host;
pub(crate) mod routes;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(assignments::router())
        .merge(certificates::router())
        .merge(deployments::router())
        .merge(ingress_status::router())
        .merge(preview::router())
        .merge(resolve_host::router())
        .merge(routes::router())
}
