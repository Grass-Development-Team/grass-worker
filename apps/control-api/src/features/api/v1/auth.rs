pub(crate) mod csrf;
pub(crate) mod email;
pub(crate) mod login;
pub(crate) mod logout;
pub(crate) mod mfa;
pub(crate) mod password;
pub(crate) mod providers;
pub(crate) mod register;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .merge(csrf::router())
        .merge(email::router())
        .merge(login::router())
        .merge(logout::router())
        .merge(mfa::router())
        .merge(password::router())
        .merge(providers::router())
        .merge(register::router())
}
