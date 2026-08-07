use axum::{extract::State, response::IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde_json::json;

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: "auth.logout.no_cache",
        message: "cache service not available".to_owned(),
    })?;
    grass_session::revoke_session(cache, &session.session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.logout.revoke_session",
            source,
        })?;

    let (configured_secure, development_enabled) = {
        let config = state.config.read().unwrap();
        (config.session.cookie_secure, config.development_enabled())
    };
    let jar = jar.add(removal_cookie(configured_secure, development_enabled));

    Ok((jar, ok_response(json!({"message": "logged out"}))))
}

fn removal_cookie(configured_secure: bool, development_enabled: bool) -> Cookie<'static> {
    let secure = configured_secure && !development_enabled;
    let mut clear_cookie = Cookie::new("session_id", "");
    clear_cookie.set_path("/api");
    clear_cookie.set_http_only(true);
    clear_cookie.set_secure(secure);
    clear_cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    if secure {
        clear_cookie.set_partitioned(true);
    }
    clear_cookie.make_removal();
    clear_cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_cookie_is_not_secure_in_development_mode() {
        let production = removal_cookie(true, false);
        assert_eq!(production.secure(), Some(true));
        assert_eq!(production.partitioned(), Some(true));

        let cookie = removal_cookie(true, true);

        assert_eq!(cookie.path(), Some("/api"));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(false));
        assert_eq!(
            cookie.same_site(),
            Some(axum_extra::extract::cookie::SameSite::Strict)
        );
        assert_eq!(cookie.partitioned(), None);
        assert_eq!(cookie.max_age(), Some(time::Duration::ZERO));
    }
}
