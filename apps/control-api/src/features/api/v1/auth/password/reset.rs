use axum::{Json, extract::State, response::IntoResponse};
use sea_orm::TransactionTrait;
use serde::Deserialize;

use crate::{
    domain::{authentication, users},
    infra::{
        database::entity::AuthTokenKind,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/password/reset", axum::routing::post(reset))
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
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    authentication::consume_auth_token(&transaction, token, AuthTokenKind::PasswordReset)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Validation {
            op: OP,
            message: "password reset token is invalid or expired".to_owned(),
        })?;
    set_password(&transaction, user_id, &body.password, OP).await?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(ResetResponse { reset: true }))
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

async fn set_password<C: sea_orm::ConnectionTrait + TransactionTrait>(
    db: &C,
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

#[derive(serde::Serialize)]
struct ResetResponse {
    reset: bool,
}
