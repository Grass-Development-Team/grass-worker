//! Shared MFA enrollment, code verification, rate limits and audit events.
use grass_cache::Cache;
use rand::{Rng, rngs::OsRng};
use serde_json::json;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{authentication, platform_mail, settings, users},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, MfaFactorKind, user, user_mfa_factor},
        error::AppError,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

const ATTEMPT_WINDOW: StdDuration = StdDuration::from_secs(10 * 60);

const CODE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

pub(crate) async fn challenge_user(
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

fn code_key(scope: &str, factor_id: Uuid) -> String {
    format!(
        "auth:mfa:code:{}:{factor_id}",
        grass_token::hash_token(scope)
    )
}

pub(crate) async fn enforce_attempt_limit(
    cache: &grass_cache::CacheStore,
    scope: &str,
    op: &'static str,
) -> Result<(), AppError> {
    if !cache
        .consume_rate_limit(
            &format!("auth:mfa:attempt:{}", grass_token::hash_token(scope)),
            5,
            ATTEMPT_WINDOW,
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

pub(crate) async fn factor_for_user(
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

pub(crate) async fn record_factor_audit(
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

pub(crate) async fn send_email_code(
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

async fn setting_string(
    db: &sea_orm::DatabaseConnection,
    key: &str,
) -> anyhow::Result<Option<String>> {
    Ok(settings::get_setting(db, key)
        .await?
        .and_then(|setting| setting.value.as_str().map(str::to_owned)))
}

pub(crate) async fn start_email_factor(
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

pub(crate) async fn start_totp(
    state: &ControlApiState,
    user: &user::Model,
    op: &'static str,
) -> Result<TotpEnrollment, AppError> {
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
    Ok(TotpEnrollment {
        factor,
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

pub(crate) async fn verified_factor(
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

pub(crate) async fn verify_factor_code(
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

pub(crate) struct TotpEnrollment {
    pub(crate) factor: user_mfa_factor::Model,
    pub(crate) secret: String,
    pub(crate) otpauth_uri: String,
}
