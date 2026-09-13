pub(crate) mod approve;
pub(crate) mod reject;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(approve::router())
        .merge(reject::router())
}
