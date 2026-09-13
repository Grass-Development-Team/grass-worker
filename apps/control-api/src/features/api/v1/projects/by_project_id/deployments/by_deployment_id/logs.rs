pub(crate) mod ws;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(ws::router())
}
