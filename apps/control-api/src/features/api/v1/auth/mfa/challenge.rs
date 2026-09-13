use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        authentication,
        login_challenges::{ChallengeMode, challenge_authenticated_user, load_challenge},
    },
    infra::{
        database::entity::user_mfa_factor,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/challenge", axum::routing::post(challenge_status))
}

#[derive(Deserialize)]
pub struct ChallengeRequest {
    pub challenge_token: String,
    #[serde(rename = "factor_id")]
    _factor_id: Option<Uuid>,
}

pub async fn challenge_status(
    State(state): State<ControlApiState>,
    Json(body): Json<ChallengeRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.mfa.status";
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let challenge = load_challenge(cache, body.challenge_token.trim(), OP).await?;
    let user = challenge_authenticated_user(&state, &challenge, OP).await?;
    let db = state.try_database().unwrap();
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let factors = authentication::verified_mfa_factors(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .into_iter()
        .filter(|factor| policy.allows(&factor.kind))
        .collect::<Vec<_>>();
    Ok(ok_response(ChallengeStatusResponse {
        mfa_required: challenge.mode == ChallengeMode::Verify,
        mfa_enrollment_required: challenge.mode == ChallengeMode::Enroll,
        challenge_token: body.challenge_token.trim().to_owned(),
        factors: factors.iter().map(factor_view).collect::<Vec<_>>(),
        allowed_factors: policy.allowed_factors,
        return_to: challenge.return_to,
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
struct ChallengeStatusResponse {
    mfa_required: bool,
    mfa_enrollment_required: bool,
    challenge_token: String,
    factors: Vec<MfaFactorResponse>,
    allowed_factors: Vec<String>,
    return_to: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::login_challenges::LoginChallenge;

    use crate::infra::error::AppError;
    use crate::state::ControlApiState;
    #[tokio::test]
    async fn a_password_reset_invalidates_a_pending_login_challenge() {
        let mut user = crate::infra::http::middlewares::session::tests::active_user();
        let challenge = LoginChallenge {
            user_id: user.id,
            auth_version: 1,
            mode: ChallengeMode::Verify,
            return_to: "/".into(),
        };
        user.auth_version = 2;
        let state = ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state
            .database
            .set(
                sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
                    .append_query_results([vec![user]])
                    .into_connection(),
            )
            .ok()
            .unwrap();
        assert!(matches!(
            challenge_authenticated_user(&state, &challenge, "test").await,
            Err(AppError::Unauthorized { .. })
        ));
    }
}
