use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        login_challenges::{ChallengeMode, challenge_authenticated_user, load_challenge},
        mfa::{send_email_code, start_email_factor, verified_factor},
    },
    infra::{
        database::entity::{MfaFactorKind, user_mfa_factor},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/email/send", axum::routing::post(challenge_email_send))
}

#[derive(Deserialize)]
struct ChallengeRequest {
    challenge_token: String,
    factor_id: Option<Uuid>,
}

async fn challenge_email_send(
    State(state): State<ControlApiState>,
    Json(body): Json<ChallengeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.mfa.email.send";
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let challenge_token = body.challenge_token.trim();
    let challenge = load_challenge(cache, challenge_token, OP).await?;
    let user = challenge_authenticated_user(&state, &challenge, OP).await?;
    let factor = match challenge.mode {
        ChallengeMode::Enroll => start_email_factor(&state, &user, OP).await?,
        ChallengeMode::Verify => {
            let factor_id = body.factor_id.ok_or_else(|| AppError::Validation {
                op: OP,
                message: "factor_id is required".to_owned(),
            })?;
            verified_factor(&state, user.id, factor_id, MfaFactorKind::Email, OP).await?
        }
    };
    send_email_code(&state, &user, &factor, challenge_token, OP).await?;
    Ok(ok_response(ChallengeEmailSendResponse {
        factor: factor_view(&factor),
    }))
}

fn factor_view(factor: &user_mfa_factor::Model) -> MfaFactorResponse {
    MfaFactorResponse {
        id: factor.id,
        kind: factor.kind.as_str(),
        label: factor.label.clone(),
        verified: factor.verified_at.is_some(),
        created_at: factor.created_at,
        last_used_at: factor.last_used_at,
    }
}

#[derive(serde::Serialize)]
struct MfaFactorResponse {
    id: uuid::Uuid,
    kind: &'static str,
    label: Option<String>,
    verified: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_used_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct ChallengeEmailSendResponse {
    factor: MfaFactorResponse,
}
