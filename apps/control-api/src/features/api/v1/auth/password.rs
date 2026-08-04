use axum::{Json, extract::State, response::IntoResponse};
use grass_cache::Cache;
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::AuthTokenKind,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct ForgotPasswordRequest {
    pub email: String,
}

pub async fn forgot(
    State(state): State<ControlApiState>,
    Json(body): Json<ForgotPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.password.forgot";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let mail_config = state.config.read().unwrap().mail.clone();
    let rate_key = format!(
        "auth:password:forgot:{}",
        grass_token::hash_token(body.email.trim().to_ascii_lowercase().as_str())
    );
    let allowed = state
        .try_cache()
        .ok_or_else(|| AppError::Internal {
            op: OP,
            message: "cache service not available".to_owned(),
        })?
        .consume_rate_limit(&rate_key, 3, std::time::Duration::from_secs(15 * 60))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if allowed
        && mail_config.enabled()
        && let Ok(email) = grass_validator::normalize_email(&body.email)
        && let Some(user) = users::get_user_by_email(db, &email)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
        && user.email_verified_at.is_some()
    {
        let token = authentication::create_auth_token(
            db,
            user.id,
            AuthTokenKind::PasswordReset,
            time::Duration::hours(1),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        platform_mail::send_password_reset_best_effort(db, mail_config, &user.email, &token).await;
    }
    Ok(ok_response(json!({ "accepted": true })))
}

#[derive(Deserialize)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub password: String,
}

pub async fn reset(
    State(state): State<ControlApiState>,
    Json(body): Json<ResetPasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.password.reset";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    policy
        .validate_password(&body.password)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    let token = body.token.trim();
    let user_id = authentication::auth_token_user(db, token, AuthTokenKind::PasswordReset)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "password reset token is invalid or expired".to_owned(),
        })?;
    ensure_not_reused(db, user_id, &body.password, policy.history_count, OP).await?;
    authentication::consume_auth_token(db, token, AuthTokenKind::PasswordReset)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "password reset token is invalid or expired".to_owned(),
        })?;
    set_password(db, user_id, &body.password, OP).await?;
    Ok(ok_response(json!({ "reset": true })))
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub password: String,
}

pub async fn change(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.password.change";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    if !authentication::verify_password_for_user(db, data.user_id, &body.current_password)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "current password is incorrect".to_owned(),
        });
    }
    let policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    policy
        .validate_password(&body.password)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    ensure_not_reused(db, data.user_id, &body.password, policy.history_count, OP).await?;
    set_password(db, data.user_id, &body.password, OP).await?;
    Ok(ok_response(json!({ "changed": true })))
}

async fn ensure_not_reused(
    db: &sea_orm::DatabaseConnection,
    user_id: uuid::Uuid,
    password: &str,
    count: usize,
    op: &'static str,
) -> Result<(), AppError> {
    if authentication::password_was_used_recently(db, user_id, password, count)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::Validation {
            op,
            message: "password was used recently".to_owned(),
        });
    }
    Ok(())
}

async fn set_password(
    db: &sea_orm::DatabaseConnection,
    user_id: uuid::Uuid,
    password: &str,
    op: &'static str,
) -> Result<(), AppError> {
    let hash = grass_crypto::hash_password(password).map_err(|error| AppError::Internal {
        op,
        message: format!("password hashing failed: {error}"),
    })?;
    users::set_password(db, user_id, hash)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })
}
