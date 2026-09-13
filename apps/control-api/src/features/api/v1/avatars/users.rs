pub(crate) mod by_user_id;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(by_user_id::router())
}
