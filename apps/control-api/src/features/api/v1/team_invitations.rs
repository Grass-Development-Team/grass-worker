pub(crate) mod accept;
pub(crate) mod preflight;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(accept::router())
        .merge(preflight::router())
}
