use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{authentication, users},
    infra::{
        database::entity::{AuthTokenKind, user},
        error::{AppError, ok_response},
        http::session_cookies::session_cookie,
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
    let issued = crate::domain::authenticated_sessions::issue(state, cache, user).await?;
    let config = state.config.read().unwrap();
    let cookie = session_cookie(
        issued.session_id,
        config.session.cookie_secure,
        config.development_enabled(),
        issued.ttl,
    );
    Ok((jar.add(cookie), issued.csrf_token))
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
