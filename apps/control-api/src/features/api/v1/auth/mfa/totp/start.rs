use axum::{Json, extract::State, response::IntoResponse};
use grass_cache::Cache;
use serde::{Deserialize, Serialize};
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use crate::{
    domain::{authentication, settings, users},
    infra::{
        database::entity::{MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/totp/start", axum::routing::post(challenge_totp_start))
}

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
    Ok(ok_response(enrollment))
}

async fn start_totp(
    state: &ControlApiState,
    user: &user::Model,
    op: &'static str,
) -> Result<TotpEnrollmentResponse, AppError> {
    let db = state.try_database().unwrap();
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    if !policy.allows(&MfaFactorKind::Totp) {
        return Err(AppError::Forbidden {
            op,
            message: "TOTP is not allowed by the platform MFA policy".to_owned(),
        });
    }
    let secret = Secret::generate_secret()
        .to_bytes()
        .map_err(|error| AppError::Internal {
            op,
            message: format!("TOTP secret generation failed: {error}"),
        })?;
    let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
    let factor = authentication::start_mfa_factor(
        db,
        user.id,
        MfaFactorKind::Totp,
        Some(secret.clone()),
        &platform_secret,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    let issuer = setting_string(db, "site.name")
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .unwrap_or_else(|| "Grass Worker".to_owned())
        .replace(':', " ");
    let totp = totp(secret, Some(issuer), user.email.clone(), op)?;
    Ok(TotpEnrollmentResponse {
        factor: factor_view(&factor),
        secret: totp.get_secret_base32(),
        otpauth_uri: totp.get_url(),
    })
}

fn totp(
    secret: Vec<u8>,
    issuer: Option<String>,
    account: String,
    op: &'static str,
) -> Result<TOTP, AppError> {
    TOTP::new(Algorithm::SHA1, 6, 1, 30, secret, issuer, account).map_err(|error| {
        AppError::Internal {
            op,
            message: format!("TOTP configuration is invalid: {error}"),
        }
    })
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

async fn setting_string(
    db: &sea_orm::DatabaseConnection,
    key: &str,
) -> anyhow::Result<Option<String>> {
    Ok(settings::get_setting(db, key)
        .await?
        .and_then(|setting| setting.value.as_str().map(str::to_owned)))
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
