pub(crate) mod forgot;
pub(crate) mod reset;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(forgot::router())
        .merge(reset::router())
}
