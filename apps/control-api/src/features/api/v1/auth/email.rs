pub(crate) mod resend;
pub(crate) mod verify;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(resend::router())
        .merge(verify::router())
}
