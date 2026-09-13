use axum::{extract::State, response::IntoResponse};
use grass_cache::Cache;
use rand::{Rng, rngs::OsRng};
use uuid::Uuid;

use crate::{
    domain::{authentication, platform_mail, users},
    infra::{
        database::entity::{MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};
use std::time::Duration as StdDuration;

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/me/mfa/email/start",
        axum::routing::post(account_email_start),
    )
}

const CODE_TTL: StdDuration = StdDuration::from_secs(10 * 60);

fn code_key(scope: &str, factor_id: Uuid) -> String {
    format!(
        "auth:mfa:code:{}:{factor_id}",
        grass_token::hash_token(scope)
    )
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
    Ok(ok_response(AccountEmailStartResponse {
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
struct AccountEmailStartResponse {
    factor: MfaFactorResponse,
}
