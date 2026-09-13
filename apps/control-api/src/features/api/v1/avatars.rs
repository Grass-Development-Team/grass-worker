pub(crate) mod teams;
pub(crate) mod users;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(teams::router())
        .merge(users::router())
}
