pub(crate) mod avatar_webp;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(avatar_webp::router())
}
