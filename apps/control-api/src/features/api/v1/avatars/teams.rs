pub(crate) mod by_team_id;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().merge(by_team_id::router())
}
