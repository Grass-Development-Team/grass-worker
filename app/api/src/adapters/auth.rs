#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthService;

pub fn clear_session_cookie(jar: axum_extra::extract::CookieJar) -> axum_extra::extract::CookieJar {
    let mut cookie = axum_extra::extract::cookie::Cookie::build((
        crate::domain::auth::SESSION_COOKIE_NAME,
        "",
    ))
    .path("/")
    .build();
    cookie.make_removal();
    jar.add(cookie)
}
