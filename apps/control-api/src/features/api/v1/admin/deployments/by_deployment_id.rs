pub(crate) mod republish;
pub(crate) mod review;
pub(crate) mod withdraw;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(republish::router())
        .merge(review::router())
        .merge(withdraw::router())
}
