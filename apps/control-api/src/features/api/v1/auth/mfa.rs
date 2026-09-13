pub(crate) mod challenge;
pub(crate) mod email;
pub(crate) mod totp;
pub(crate) mod verify;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(challenge::router())
        .merge(email::router())
        .merge(totp::router())
        .merge(verify::router())
}
