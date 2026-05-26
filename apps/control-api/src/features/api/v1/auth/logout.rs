use axum::{extract::State, response::IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde_json::json;

use crate::{
    infra::error::{AppError, ok_response},
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    if let Some(session_id) = jar.get("session_id").map(|c| c.value().to_owned()) {
        if let Some(conn) = state.try_redis() {
            let mut conn = conn.clone();
            let _ = grass_session::revoke_session(&mut conn, &session_id).await;
        }
    }

    let mut clear_cookie = Cookie::new("session_id", "");
    clear_cookie.set_path("/api");
    clear_cookie.set_http_only(true);
    clear_cookie.make_removal();
    let jar = jar.add(clear_cookie);

    Ok((jar, ok_response(json!({"message": "logged out"}))))
}
