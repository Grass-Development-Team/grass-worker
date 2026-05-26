use axum::{Json, extract::State, response::IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use serde_json::json;
use time::Duration;

use crate::{
    domain::users,
    infra::{
        error::{AppError, ok_response},
        http::csrf,
    },
    state::ControlApiState,
};

const SESSION_TTL_SECONDS: u64 = 2_592_000;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "auth.login.no_database",
        message: "database not available".to_owned(),
    })?;

    let redis_conn = state.try_redis().ok_or_else(|| AppError::Internal {
        op: "auth.login.no_redis",
        message: "session service not available".to_owned(),
    })?;

    let email = body.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::Validation {
            op: "auth.login.invalid_email",
            message: "invalid email address".to_owned(),
        });
    }

    if body.password.is_empty() {
        return Err(AppError::Validation {
            op: "auth.login.empty_password",
            message: "password is required".to_owned(),
        });
    }

    let result = users::verify_user_password(db, &email, &body.password)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.verify",
            source,
        })?;

    if !result.password_ok {
        return Err(AppError::Unauthorized {
            op: "auth.login.invalid_credentials",
            message: "invalid email or password".to_owned(),
        });
    }

    let mut conn = redis_conn.clone();
    let session_id = grass_session::create_session(&mut conn, result.user.id, SESSION_TTL_SECONDS)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.create_session",
            source,
        })?;

    let csrf_token = csrf::generate_csrf_token(&mut conn, &session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.csrf_token",
            source,
        })?;

    let cookie_secure = state.config.read().unwrap().session.cookie_secure;
    let mut cookie = Cookie::new("session_id", session_id);
    cookie.set_path("/api");
    cookie.set_http_only(true);
    if cookie_secure {
        cookie.set_secure(true);
    }
    cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    cookie.set_max_age(Duration::seconds(SESSION_TTL_SECONDS as i64));

    let session_jar = jar.add(cookie);

    Ok((
        session_jar,
        ok_response(json!({
            "user": {
                "id": result.user.id,
                "email": result.user.email,
                "display_name": result.user.display_name,
            },
            "csrf_token": csrf_token,
        })),
    ))
}
