use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        login_challenges::{ChallengeMode, challenge_authenticated_user, load_challenge},
        mfa::start_totp,
    },
    infra::{
        database::entity::user_mfa_factor,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/totp/start", axum::routing::post(challenge_totp_start))
}

#[derive(Deserialize)]
pub struct ChallengeRequest {
    pub challenge_token: String,
    #[serde(rename = "factor_id")]
    _factor_id: Option<Uuid>,
}

pub async fn challenge_totp_start(
    State(state): State<ControlApiState>,
    Json(body): Json<ChallengeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.mfa.totp.start";
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let challenge = load_challenge(cache, body.challenge_token.trim(), OP).await?;
    if challenge.mode != ChallengeMode::Enroll {
        return Err(AppError::Conflict {
            op: OP,
            message: "this challenge does not permit factor enrollment".to_owned(),
        });
    }
    let user = challenge_authenticated_user(&state, &challenge, OP).await?;
    let enrollment = start_totp(&state, &user, OP).await?;
    Ok(ok_response(TotpEnrollmentResponse {
        factor: factor_view(&enrollment.factor),
        secret: enrollment.secret,
        otpauth_uri: enrollment.otpauth_uri,
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
struct TotpEnrollmentResponse {
    factor: MfaFactorResponse,
    secret: String,
    otpauth_uri: String,
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
