use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde::Deserialize;
use std::time::Duration;
use uuid::Uuid;

use crate::{
    domain::{authentication, users},
    infra::{
        database::entity::{AuthTokenKind, user},
        error::{AppError, ok_response},
        http::middlewares::csrf,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/email/verify", axum::routing::post(verify))
}

fn user_data(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        avatar_url: user_avatar_url(user.id, user.avatar_version),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
    }
}

#[derive(Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

pub async fn verify(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(body): Json<VerifyEmailRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "auth.email.verify";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let user_id =
        authentication::consume_auth_token(db, body.token.trim(), AuthTokenKind::EmailVerification)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .ok_or_else(|| AppError::Validation {
                op: OP,
                message: "email verification token is invalid or expired".to_owned(),
            })?;
    let user = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let mut active: user::ActiveModel = user.into();
    active.email_verified_at = Set(Some(time::OffsetDateTime::now_utc()));
    let user = active
        .update(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    authenticated_response(&state, cache, jar, user).await
}

pub(crate) async fn authenticated_response(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: crate::infra::database::entity::user::Model,
) -> Result<Response, AppError> {
    let (session_jar, csrf_token) = create_authenticated_session(state, cache, jar, &user).await?;

    Ok((
        session_jar,
        ok_response(AuthenticatedResponse {
            user: user_data(&user),
            csrf_token,
        }),
    )
        .into_response())
}

pub(crate) async fn create_authenticated_session(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: &crate::infra::database::entity::user::Model,
) -> Result<(CookieJar, String), AppError> {
    if let Some(db) = state.try_database() {
        users::update_last_login(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "auth.login.update_last_login",
                source,
            })?;
    }
    let (cookie_secure, development_enabled, session_ttl) = {
        let config = state.config.read().unwrap();
        (
            config.session.cookie_secure,
            config.development_enabled(),
            Duration::from_secs(config.session.session_ttl_seconds),
        )
    };
    let session_id = grass_session::create_session(cache, user.id, user.auth_version, session_ttl)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.create_session",
            source,
        })?;

    let csrf_token = csrf::generate_csrf_token(cache, &session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.csrf_token",
            source,
        })?;

    Ok((
        jar.add(session_cookie(
            session_id,
            cookie_secure,
            development_enabled,
            session_ttl,
        )),
        csrf_token,
    ))
}

fn session_cookie(
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

pub(crate) fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    platform_role: &'static str,
    email_verified: bool,
}

#[derive(serde::Serialize)]
struct AuthenticatedResponse {
    user: UserResponse,
    csrf_token: String,
}
