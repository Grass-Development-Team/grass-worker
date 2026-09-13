use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_cache::Cache;
use serde::Deserialize;
use serde_json::json;
use totp_rs::{Algorithm, TOTP};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::authentication,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, MfaFactorKind, user_mfa_factor},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/me/mfa/{factor_id}/confirm",
        axum::routing::post(account_confirm),
    )
}

const CHALLENGE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

fn code_key(scope: &str, factor_id: Uuid) -> String {
    format!(
        "auth:mfa:code:{}:{factor_id}",
        grass_token::hash_token(scope)
    )
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
    let transaction = audits::AuditTransaction::begin(state.try_database().unwrap())
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let factor = authentication::verify_mfa_factor(&transaction, factor)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_factor_audit(
        &transaction,
        data.user_id,
        "mfa.factor_enrolled",
        &factor.kind,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(AccountConfirmResponse {
        factor: factor_view(&factor),
    }))
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
struct AccountConfirmResponse {
    factor: MfaFactorResponse,
}
