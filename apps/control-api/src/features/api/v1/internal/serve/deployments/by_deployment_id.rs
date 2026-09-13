pub(crate) mod ssr_lease;
pub(crate) mod status;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(ssr_lease::router())
        .merge(status::router())
}
