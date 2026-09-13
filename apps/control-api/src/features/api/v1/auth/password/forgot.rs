use axum::{Json, extract::State, response::IntoResponse};
use grass_cache::Cache;
use serde::Deserialize;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::AuthTokenKind,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/password/forgot", axum::routing::post(forgot))
}

#[derive(Deserialize)]
struct ForgotPasswordRequest {
    email: String,
}

async fn forgot(
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
    Ok(ok_response(ForgotResponse { accepted: true }))
}

#[derive(serde::Serialize)]
struct ForgotResponse {
    accepted: bool,
}
