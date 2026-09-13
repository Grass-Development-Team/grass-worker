pub(crate) mod release;
pub(crate) mod renew;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(release::router())
        .merge(renew::router())
}
