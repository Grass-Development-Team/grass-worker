//! MFA login challenge issuance, policy selection and credential-version validation.
use grass_cache::Cache;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::{authentication, mfa::challenge_user},
    infra::{
        database::entity::{user, user_mfa_factor},
        error::AppError,
        http::redirects::safe_return_to,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

const CHALLENGE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChallengeMode {
    Verify,
    Enroll,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LoginChallenge {
    pub(crate) user_id: Uuid,
    #[serde(default)]
    pub(crate) auth_version: i64,
    pub(crate) mode: ChallengeMode,
    pub(crate) return_to: String,
}

pub(crate) async fn challenge_authenticated_user(
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

pub(crate) fn challenge_key(token: &str) -> String {
    format!("auth:mfa:challenge:{}", grass_token::hash_token(token))
}

async fn create_challenge(
    cache: &grass_cache::CacheStore,
    user_id: Uuid,
    auth_version: i64,
    mode: ChallengeMode,
    return_to: String,
    op: &'static str,
) -> Result<String, AppError> {
    let token = grass_token::generate_token();
    cache
        .set(
            &challenge_key(&token),
            &serde_json::to_string(&LoginChallenge {
                user_id,
                auth_version,
                mode,
                return_to,
            })
            .map_err(|error| AppError::Internal {
                op,
                message: format!("MFA challenge serialization failed: {error}"),
            })?,
            CHALLENGE_TTL,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    Ok(token)
}

pub(crate) async fn load_challenge(
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

pub(crate) async fn begin(
    state: &ControlApiState,
    user: &user::Model,
    return_to: Option<&str>,
) -> Result<Option<ChallengeOffer>, AppError> {
    const OP: &str = "auth.mfa.begin";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let factors = authentication::verified_mfa_factors(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .into_iter()
        .filter(|factor| policy.allows(&factor.kind))
        .collect::<Vec<_>>();
    let user_policy = authentication::user_mfa_policy(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = policy.requirements_for(&user_policy, &user.platform_role);
    let mode = if !factors.is_empty() && requirements.met_by(&factors) {
        Some(ChallengeMode::Verify)
    } else if requirements.is_enforced() {
        Some(ChallengeMode::Enroll)
    } else if !factors.is_empty() {
        Some(ChallengeMode::Verify)
    } else {
        None
    };
    let Some(mode) = mode else {
        return Ok(None);
    };
    let return_to = safe_return_to(return_to);
    let token = create_challenge(
        cache,
        user.id,
        user.auth_version,
        mode,
        return_to.clone(),
        OP,
    )
    .await?;
    Ok(Some(ChallengeOffer {
        mode,
        challenge_token: token,
        factors,
        allowed_factors: policy.allowed_factors.clone(),
        return_to,
    }))
}

pub(crate) struct ChallengeOffer {
    pub(crate) mode: ChallengeMode,
    pub(crate) challenge_token: String,
    pub(crate) factors: Vec<user_mfa_factor::Model>,
    pub(crate) allowed_factors: Vec<String>,
    pub(crate) return_to: String,
}
