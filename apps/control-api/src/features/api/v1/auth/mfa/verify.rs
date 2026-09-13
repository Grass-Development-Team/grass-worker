use axum::{
    Json,
    extract::State,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use grass_cache::Cache;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use totp_rs::{Algorithm, TOTP};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{authentication, users},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
        http::middlewares::csrf,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

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
    if let Some(db) = state.try_database() {
        users::update_last_login(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "auth.login.update_last_login",
                source,
            })?;
    }
    let (cookie_secure, development_enabled, session_ttl) = {
        let config = state.config.read().unwrap();
        (
            config.session.cookie_secure,
            config.development_enabled(),
            Duration::from_secs(config.session.session_ttl_seconds),
        )
    };
    let session_id = grass_session::create_session(cache, user.id, user.auth_version, session_ttl)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.create_session",
            source,
        })?;

    let csrf_token = csrf::generate_csrf_token(cache, &session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.csrf_token",
            source,
        })?;

    Ok((
        jar.add(session_cookie(
            session_id,
            cookie_secure,
            development_enabled,
            session_ttl,
        )),
        csrf_token,
    ))
}

fn session_cookie(
    session_id: impl Into<String>,
    configured_secure: bool,
    development_enabled: bool,
    session_ttl: Duration,
) -> Cookie<'static> {
    let secure = configured_secure && !development_enabled;
    let mut cookie = Cookie::new("session_id", session_id.into());
    cookie.set_path("/api");
    cookie.set_http_only(true);
    cookie.set_secure(secure);
    if secure {
        cookie.set_partitioned(true);
    }
    cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    cookie.set_max_age(time::Duration::seconds(session_ttl.as_secs() as i64));
    cookie
}

const CHALLENGE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

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

async fn enforce_attempt_limit(
    cache: &grass_cache::CacheStore,
    scope: &str,
    op: &'static str,
) -> Result<(), AppError> {
    if !cache
        .consume_rate_limit(
            &format!("auth:mfa:attempt:{}", grass_token::hash_token(scope)),
            5,
            CHALLENGE_TTL,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::TooManyRequests {
            op,
            message: "too many MFA attempts".to_owned(),
        });
    }
    Ok(())
}

async fn verify_factor_code(
    state: &ControlApiState,
    factor: &user_mfa_factor::Model,
    scope: &str,
    code: &str,
    op: &'static str,
) -> Result<(), AppError> {
    let valid = match factor.kind {
        MfaFactorKind::Totp => {
            let current_step = time::OffsetDateTime::now_utc().unix_timestamp() / 30;
            if factor
                .last_used_at
                .is_some_and(|last_used| last_used.unix_timestamp() / 30 == current_step)
            {
                return Err(AppError::Unauthorized {
                    op,
                    message: "verification code was already used".to_owned(),
                });
            }
            let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
            let secret =
                authentication::decrypt_mfa_secret(&secret_key, factor).map_err(|error| {
                    AppError::Internal {
                        op,
                        message: format!("MFA secret could not be decrypted: {error}"),
                    }
                })?;
            totp(secret, None, String::new(), op)?
                .check_current(code)
                .unwrap_or(false)
        }
        MfaFactorKind::Email => {
            let cache = state.try_cache().unwrap();
            let key = code_key(scope, factor.id);
            let valid = cache
                .get(&key)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?
                .is_some_and(|hash| hash == grass_token::hash_token(code));
            if valid {
                cache
                    .delete(&key)
                    .await
                    .map_err(|source| AppError::Infrastructure { op, source })?;
            }
            valid
        }
    };
    if !valid {
        return Err(AppError::Unauthorized {
            op,
            message: "verification code is invalid or expired".to_owned(),
        });
    }
    Ok(())
}

async fn record_factor_audit(
    db: &impl audits::AuditConnection,
    user_id: Uuid,
    action: &str,
    kind: &MfaFactorKind,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(user_id),
            actor_node_id: None,
            team_id: None,
            action: action.to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(user_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "factor_kind": kind.as_str() }),
        },
    )
    .await
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
