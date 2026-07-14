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

    let mut clear_cookie = Cookie::new("session_id", "");
    clear_cookie.set_path("/api");
    clear_cookie.set_http_only(true);
    let secure = state.config.read().unwrap().session.cookie_secure;
    clear_cookie.set_secure(secure);
    clear_cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    if secure {
        clear_cookie.set_partitioned(true);
    }
    clear_cookie.make_removal();
    let jar = jar.add(clear_cookie);

    Ok((jar, ok_response(json!({"message": "logged out"}))))
}
