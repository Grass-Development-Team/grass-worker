use std::time::Duration as StdDuration;

use axum::{
    Json,
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use grass_cache::Cache;
use rand::{Rng, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        authentication, platform_mail, settings, users,
    },
    infra::{
        database::entity::{AuditEventResult, MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
        http::extractors::Session,
        http::timestamps::ts,
    },
    state::ControlApiState,
};

const CHALLENGE_TTL: StdDuration = StdDuration::from_secs(10 * 60);
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
    mode: ChallengeMode,
    return_to: String,
}

pub async fn begin_login(
    state: &ControlApiState,
    user: &user::Model,
    return_to: Option<&str>,
) -> Result<Option<Response>, AppError> {
    Ok(begin_login_payload(state, user, return_to)
        .await?
        .map(|payload| ok_response(payload).into_response()))
}

pub async fn begin_login_payload(
    state: &ControlApiState,
    user: &user::Model,
    return_to: Option<&str>,
) -> Result<Option<serde_json::Value>, AppError> {
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
    let return_to = super::oidc::safe_return_to(return_to);
    let token = create_challenge(cache, user.id, mode, return_to.clone(), OP).await?;
    Ok(Some(json!({
        "mfa_required": mode == ChallengeMode::Verify,
        "mfa_enrollment_required": mode == ChallengeMode::Enroll,
        "challenge_token": token,
        "factors": factors.iter().map(factor_view).collect::<Vec<_>>(),
        "allowed_factors": policy.allowed_factors,
        "return_to": return_to,
    })))
}

async fn create_challenge(
    cache: &grass_cache::CacheStore,
    user_id: Uuid,
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
    let user = challenge_user(&state, challenge.user_id, OP).await?;
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
    Ok(ok_response(json!({
        "mfa_required": challenge.mode == ChallengeMode::Verify,
        "mfa_enrollment_required": challenge.mode == ChallengeMode::Enroll,
        "challenge_token": body.challenge_token.trim(),
        "factors": factors.iter().map(factor_view).collect::<Vec<_>>(),
        "allowed_factors": policy.allowed_factors,
        "return_to": challenge.return_to,
    })))
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
    let user = challenge_user(&state, challenge.user_id, OP).await?;
    let enrollment = start_totp(&state, &user, OP).await?;
    Ok(ok_response(enrollment))
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
    let user = challenge_user(&state, challenge.user_id, OP).await?;
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
    Ok(ok_response(json!({ "factor": factor_view(&factor) })))
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
    let user = challenge_user(&state, challenge.user_id, OP).await?;
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
    let enrolled = factor.verified_at.is_none();
    let factor_kind = factor.kind.clone();
    if enrolled {
        authentication::verify_mfa_factor(db, factor)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    } else {
        authentication::mark_mfa_factor_used(db, factor)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    if enrolled {
        record_factor_audit(db, user.id, "mfa.factor_enrolled", &factor_kind).await;
    }
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
            return Ok(ok_response(json!({
                "mfa_required": false,
                "mfa_enrollment_required": true,
                "challenge_token": token,
                "factors": factors.iter().map(factor_view).collect::<Vec<_>>(),
                "allowed_factors": policy.allowed_factors,
                "return_to": challenge.return_to,
            }))
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
    super::login::authenticated_response(&state, cache, jar, user).await
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

pub async fn security(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.security";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let user = users::get_user_by_id(db, data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let factors = authentication::verified_mfa_factors(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let password_policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let user_policy = authentication::user_mfa_policy(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = policy.requirements_for(&user_policy, &user.platform_role);
    Ok(ok_response(json!({
        "email_verified": user.email_verified_at.is_some(),
        "factors": factors.iter().map(factor_view).collect::<Vec<_>>(),
        "allowed_factors": policy.allowed_factors,
        "mfa_required": requirements.is_enforced(),
        "mfa_requirements": {
            "minimum_factors": requirements.minimum_factors,
            "required_factors": requirements.required_factors.iter().map(MfaFactorKind::as_str).collect::<Vec<_>>(),
        },
        "mfa_policy": user_policy,
        "password_policy": password_policy,
        "mail_available": state.config.read().unwrap().mail.enabled(),
    })))
}

pub async fn account_totp_start(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.mfa.totp.start";
    let user = challenge_user(&state, data.user_id, OP).await?;
    Ok(ok_response(start_totp(&state, &user, OP).await?))
}

pub async fn account_email_start(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.mfa.email.start";
    let user = challenge_user(&state, data.user_id, OP).await?;
    let factor = start_email_factor(&state, &user, OP).await?;
    let scope = format!("account:{}", user.id);
    send_email_code(&state, &user, &factor, &scope, OP).await?;
    Ok(ok_response(json!({ "factor": factor_view(&factor) })))
}

#[derive(Deserialize)]
pub struct ConfirmFactorRequest {
    pub code: String,
}

pub async fn account_confirm(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(factor_id): Path<Uuid>,
    Json(body): Json<ConfirmFactorRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.mfa.confirm";
    let factor = factor_for_user(&state, data.user_id, factor_id, OP).await?;
    let scope = format!("account:{}", data.user_id);
    enforce_attempt_limit(state.try_cache().unwrap(), &scope, OP).await?;
    verify_factor_code(&state, &factor, &scope, body.code.trim(), OP).await?;
    let factor = authentication::verify_mfa_factor(state.try_database().unwrap(), factor)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_factor_audit(
        state.try_database().unwrap(),
        data.user_id,
        "mfa.factor_enrolled",
        &factor.kind,
    )
    .await;
    Ok(ok_response(json!({ "factor": factor_view(&factor) })))
}

pub async fn account_delete(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(factor_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.mfa.delete";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let user = challenge_user(&state, data.user_id, OP).await?;
    let factor = factor_for_user(&state, user.id, factor_id, OP).await?;
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if factor.verified_at.is_some() {
        let user_policy = authentication::user_mfa_policy(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let requirements = policy.requirements_for(&user_policy, &user.platform_role);
        let remaining = authentication::verified_mfa_factors(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .into_iter()
            .filter(|candidate| candidate.id != factor.id && policy.allows(&candidate.kind))
            .collect::<Vec<_>>();
        if requirements.is_enforced() && !requirements.met_by(&remaining) {
            return Err(AppError::Conflict {
                op: OP,
                message: "the effective MFA policy requires more enrolled factors".to_owned(),
            });
        }
    }
    authentication::delete_mfa_factor(db, user.id, factor.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_factor_audit(db, user.id, "mfa.factor_removed", &factor.kind).await;
    Ok(ok_response(json!({ "deleted": true })))
}

async fn record_factor_audit(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    action: &str,
    kind: &MfaFactorKind,
) {
    let _ = audits::create_platform_audit_event(
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
    .await;
}

async fn start_totp(
    state: &ControlApiState,
    user: &user::Model,
    op: &'static str,
) -> Result<serde_json::Value, AppError> {
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
    Ok(json!({
        "factor": factor_view(&factor),
        "secret": totp.get_secret_base32(),
        "otpauth_uri": totp.get_url(),
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

fn factor_view(factor: &user_mfa_factor::Model) -> serde_json::Value {
    json!({
        "id": factor.id,
        "kind": factor.kind.as_str(),
        "label": factor.label,
        "verified": factor.verified_at.is_some(),
        "created_at": ts(factor.created_at),
        "last_used_at": ts(factor.last_used_at),
    })
}

async fn setting_string(
    db: &sea_orm::DatabaseConnection,
    key: &str,
) -> anyhow::Result<Option<String>> {
    Ok(settings::get_setting(db, key)
        .await?
        .and_then(|setting| setting.value.as_str().map(str::to_owned)))
}
