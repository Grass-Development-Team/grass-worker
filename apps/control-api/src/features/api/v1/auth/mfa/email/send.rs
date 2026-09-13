use axum::{Json, extract::State, response::IntoResponse};
use grass_cache::Cache;
use rand::{Rng, rngs::OsRng};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::{MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/email/send", axum::routing::post(challenge_email_send))
}

const CODE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ChallengeMode {
    Verify,
    Enroll,
}

#[derive(Debug, Deserialize, Serialize)]
struct LoginChallenge {
    user_id: Uuid,
    #[serde(default)]
    auth_version: i64,
    mode: ChallengeMode,
    return_to: String,
}

fn challenge_key(token: &str) -> String {
    format!("auth:mfa:challenge:{}", grass_token::hash_token(token))
}

fn code_key(scope: &str, factor_id: Uuid) -> String {
    format!(
        "auth:mfa:code:{}:{factor_id}",
        grass_token::hash_token(scope)
    )
}

async fn load_challenge(
    cache: &grass_cache::CacheStore,
    token: &str,
    op: &'static str,
) -> Result<LoginChallenge, AppError> {
    cache
        .get(&challenge_key(token))
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|value| serde_json::from_str(&value).ok())
        .ok_or_else(|| AppError::Unauthorized {
            op,
            message: "MFA challenge is invalid or expired".to_owned(),
        })
}

#[derive(Deserialize)]
pub struct ChallengeRequest {
    pub challenge_token: String,
    pub factor_id: Option<Uuid>,
}

pub async fn challenge_email_send(
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

async fn start_email_factor(
    state: &ControlApiState,
    user: &user::Model,
    op: &'static str,
) -> Result<user_mfa_factor::Model, AppError> {
    if user.email_verified_at.is_none() || !state.config.read().unwrap().mail.enabled() {
        return Err(AppError::Conflict {
            op,
            message: "email MFA requires a verified email and enabled mail transport".to_owned(),
        });
    }
    let db = state.try_database().unwrap();
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    if !policy.allows(&MfaFactorKind::Email) {
        return Err(AppError::Forbidden {
            op,
            message: "email is not allowed by the platform MFA policy".to_owned(),
        });
    }
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    authentication::start_mfa_factor(db, user.id, MfaFactorKind::Email, None, &platform_secret)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })
}

async fn send_email_code(
    state: &ControlApiState,
    user: &user::Model,
    factor: &user_mfa_factor::Model,
    scope: &str,
    op: &'static str,
) -> Result<(), AppError> {
    if !state
        .try_cache()
        .unwrap()
        .consume_rate_limit(
            &format!(
                "auth:mfa:send:{}:{}",
                grass_token::hash_token(scope),
                factor.id
            ),
            3,
            CODE_TTL,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::TooManyRequests {
            op,
            message: "too many MFA codes requested".to_owned(),
        });
    }
    let code = format!("{:06}", OsRng.gen_range(0..1_000_000_u32));
    state
        .try_cache()
        .unwrap()
        .set(
            &code_key(scope, factor.id),
            &grass_token::hash_token(&code),
            CODE_TTL,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let mail_config = state.config.read().unwrap().mail.clone();
    platform_mail::send_mfa_code_best_effort(
        state.try_database().unwrap(),
        mail_config,
        &user.email,
        &code,
    )
    .await;
    Ok(())
}

async fn challenge_user(
    state: &ControlApiState,
    user_id: Uuid,
    op: &'static str,
) -> Result<user::Model, AppError> {
    users::get_user_by_id(state.try_database().unwrap(), user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "user not found".to_owned(),
        })
}

async fn factor_for_user(
    state: &ControlApiState,
    user_id: Uuid,
    factor_id: Uuid,
    op: &'static str,
) -> Result<user_mfa_factor::Model, AppError> {
    authentication::mfa_factor(state.try_database().unwrap(), user_id, factor_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "MFA factor not found".to_owned(),
        })
}

async fn verified_factor(
    state: &ControlApiState,
    user_id: Uuid,
    factor_id: Uuid,
    kind: MfaFactorKind,
    op: &'static str,
) -> Result<user_mfa_factor::Model, AppError> {
    let factor = factor_for_user(state, user_id, factor_id, op).await?;
    if factor.kind != kind || factor.verified_at.is_none() {
        return Err(AppError::Forbidden {
            op,
            message: "MFA factor is not available".to_owned(),
        });
    }
    Ok(factor)
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

async fn challenge_authenticated_user(
    state: &ControlApiState,
    challenge: &LoginChallenge,
    op: &'static str,
) -> Result<user::Model, AppError> {
    let user = challenge_user(state, challenge.user_id, op).await?;
    if challenge.auth_version <= 0 || challenge.auth_version != user.auth_version {
        return Err(AppError::Unauthorized {
            op,
            message: "MFA challenge is invalid or expired".to_owned(),
        });
    }
    Ok(user)
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
