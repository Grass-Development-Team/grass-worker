use axum::{Json, extract::State, response::IntoResponse};
use grass_cache::Cache;
use serde::Deserialize;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::AuthTokenKind,
        error::{AppError, ok_response},
        http::redirects::safe_return_to,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/email/resend", axum::routing::post(resend))
}

#[derive(Deserialize)]
pub struct ResendEmailRequest {
    pub email: String,
    pub return_to: Option<String>,
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
    let return_to = safe_return_to(body.return_to.as_deref());
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
        platform_mail::send_email_verification_best_effort(
            db,
            mail_config,
            &user.email,
            &token,
            Some(&return_to),
        )
        .await;
    }
    Ok(ok_response(ResendResponse { accepted: true }))
}

#[derive(serde::Serialize)]
struct ResendResponse {
    accepted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resend_request_accepts_a_local_return_destination() {
        let request: ResendEmailRequest = serde_json::from_value(serde_json::json!({
            "email": "user@example.com",
            "return_to": "/invitations/accept?token=invite-token"
        }))
        .unwrap();

        assert_eq!(
            request.return_to.as_deref(),
            Some("/invitations/accept?token=invite-token")
        );
    }
}
