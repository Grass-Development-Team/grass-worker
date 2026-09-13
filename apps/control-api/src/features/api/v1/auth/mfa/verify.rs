use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use grass_cache::Cache;
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        authentication,
        login_challenges::{
            ChallengeMode, challenge_authenticated_user, challenge_key, load_challenge,
        },
        mfa::{enforce_attempt_limit, factor_for_user, record_factor_audit, verify_factor_code},
    },
    infra::{
        database::entity::{user, user_mfa_factor},
        error::{AppError, ok_response},
        http::session_cookies::session_cookie,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/mfa/verify", axum::routing::post(challenge_verify))
}

fn user_data(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        avatar_url: user_avatar_url(user.id, user.avatar_version),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
    }
}

pub(crate) async fn authenticated_response(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: crate::infra::database::entity::user::Model,
) -> Result<Response, AppError> {
    let (session_jar, csrf_token) = create_authenticated_session(state, cache, jar, &user).await?;

    Ok((
        session_jar,
        ok_response(AuthenticatedResponse {
            user: user_data(&user),
            csrf_token,
        }),
    )
        .into_response())
}

pub(crate) async fn create_authenticated_session(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: &crate::infra::database::entity::user::Model,
) -> Result<(CookieJar, String), AppError> {
    let issued = crate::domain::authenticated_sessions::issue(state, cache, user).await?;
    let config = state.config.read().unwrap();
    let cookie = session_cookie(
        issued.session_id,
        config.session.cookie_secure,
        config.development_enabled(),
        issued.ttl,
    );
    Ok((jar.add(cookie), issued.csrf_token))
}

#[derive(Deserialize)]
pub struct VerifyChallengeRequest {
    pub challenge_token: String,
    pub factor_id: Uuid,
    pub code: String,
}

pub async fn challenge_verify(
    State(state): State<ControlApiState>,
    jar: CookieJar,
    Json(body): Json<VerifyChallengeRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "auth.mfa.verify";
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let token = body.challenge_token.trim();
    let challenge = load_challenge(cache, token, OP).await?;
    let user = challenge_authenticated_user(&state, &challenge, OP).await?;
    let factor = factor_for_user(&state, user.id, body.factor_id, OP).await?;
    let policy = authentication::mfa_policy(state.try_database().unwrap())
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if !policy.allows(&factor.kind)
        || (challenge.mode == ChallengeMode::Verify && factor.verified_at.is_none())
    {
        return Err(AppError::Forbidden {
            op: OP,
            message: "MFA factor is not available".to_owned(),
        });
    }
    enforce_attempt_limit(cache, token, OP).await?;
    verify_factor_code(&state, &factor, token, body.code.trim(), OP).await?;
    let db = state.try_database().unwrap();
    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let enrolled = factor.verified_at.is_none();
    let factor_kind = factor.kind.clone();
    if enrolled {
        authentication::verify_mfa_factor(&transaction, factor)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    } else {
        authentication::mark_mfa_factor_used(&transaction, factor)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    if enrolled {
        record_factor_audit(&transaction, user.id, "mfa.factor_enrolled", &factor_kind)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if challenge.mode == ChallengeMode::Enroll {
        let factors = authentication::verified_mfa_factors(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .into_iter()
            .filter(|candidate| policy.allows(&candidate.kind))
            .collect::<Vec<_>>();
        let user_policy = authentication::user_mfa_policy(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let requirements = policy.requirements_for(&user_policy, &user.platform_role);
        if !requirements.met_by(&factors) {
            return Ok(ok_response(ChallengeVerifyResponse {
                mfa_required: false,
                mfa_enrollment_required: true,
                challenge_token: token.to_owned(),
                factors: factors.iter().map(factor_view).collect::<Vec<_>>(),
                allowed_factors: policy.allowed_factors,
                return_to: challenge.return_to,
            })
            .into_response());
        }
    }
    cache
        .take(&challenge_key(token))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "MFA challenge was already used".to_owned(),
        })?;
    authenticated_response(&state, cache, jar, user).await
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

pub(crate) fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    platform_role: &'static str,
    email_verified: bool,
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
struct AuthenticatedResponse {
    user: UserResponse,
    csrf_token: String,
}

#[derive(serde::Serialize)]
struct ChallengeVerifyResponse {
    mfa_required: bool,
    mfa_enrollment_required: bool,
    challenge_token: String,
    factors: Vec<MfaFactorResponse>,
    allowed_factors: Vec<String>,
    return_to: String,
}
