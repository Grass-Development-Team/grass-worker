pub(crate) mod by_factor_id;
pub(crate) mod email;
pub(crate) mod totp;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(by_factor_id::router())
        .merge(email::router())
        .merge(totp::router())
}
