pub(crate) mod by_deployment_id;
pub(crate) mod claim;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(by_deployment_id::router())
        .merge(claim::router())
}
