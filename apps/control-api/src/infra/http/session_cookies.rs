//! Cookie attributes for authenticated browser sessions.
use axum_extra::extract::cookie::Cookie;
use std::time::Duration;

pub(crate) fn session_cookie(
    session_id: impl Into<String>,
    configured_secure: bool,
    development_enabled: bool,
    session_ttl: Duration,
) -> Cookie<'static> {
    let secure = configured_secure && !development_enabled;
    let mut cookie = Cookie::new("session_id", session_id.into());
    cookie.set_path("/api");
    cookie.set_http_only(true);
    cookie.set_secure(secure);
    if secure {
        cookie.set_partitioned(true);
    }
    cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    cookie.set_max_age(time::Duration::seconds(session_ttl.as_secs() as i64));
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_cookie_contains_all_security_attributes() {
        let cookie = session_cookie("session", true, false, Duration::from_secs(3600));

        assert_eq!(cookie.path(), Some("/api"));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(
            cookie.same_site(),
            Some(axum_extra::extract::cookie::SameSite::Strict)
        );
        assert_eq!(cookie.partitioned(), Some(true));
        assert_eq!(cookie.max_age(), Some(time::Duration::hours(1)));
    }

    #[test]
    fn session_cookie_is_not_secure_in_development_mode() {
        let cookie = session_cookie("session", true, true, Duration::from_secs(3600));

        assert_eq!(cookie.secure(), Some(false));
        assert_eq!(cookie.partitioned(), None);
        assert_eq!(
            cookie.same_site(),
            Some(axum_extra::extract::cookie::SameSite::Strict)
        );
    }
}
