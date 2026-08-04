use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use grass_cache::Cache;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::{AuthTokenKind, user},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

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
    super::login::authenticated_response(&state, cache, jar, user).await
}

#[derive(Deserialize)]
pub struct ResendEmailRequest {
    pub email: String,
}

pub async fn resend(
    State(state): State<ControlApiState>,
    Json(body): Json<ResendEmailRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.email.resend";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let mail_config = state.config.read().unwrap().mail.clone();
    let allowed = state
        .try_cache()
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "cache service not available".to_owned(),
        })?
        .consume_rate_limit(
            &format!(
                "auth:email:resend:{}",
                grass_token::hash_token(body.email.trim().to_ascii_lowercase().as_str())
            ),
            3,
            std::time::Duration::from_secs(15 * 60),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if allowed
        && mail_config.enabled()
        && let Ok(email) = grass_validator::normalize_email(&body.email)
        && let Some(user) = users::get_user_by_email(db, &email)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
        && user.email_verified_at.is_none()
    {
        let token = authentication::create_auth_token(
            db,
            user.id,
            AuthTokenKind::EmailVerification,
            time::Duration::hours(24),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        platform_mail::send_email_verification_best_effort(db, mail_config, &user.email, &token)
            .await;
    }
    Ok(ok_response(json!({ "accepted": true })))
}
