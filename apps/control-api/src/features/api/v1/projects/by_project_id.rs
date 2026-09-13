pub(crate) mod deployments;
pub(crate) mod serve_nodes;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(deployments::router())
        .merge(serve_nodes::router())
}
