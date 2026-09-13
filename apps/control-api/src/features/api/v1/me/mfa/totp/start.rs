use axum::{extract::State, response::IntoResponse};
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use crate::{
    domain::{authentication, settings, users},
    infra::{
        database::entity::{MfaFactorKind, user, user_mfa_factor},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/me/mfa/totp/start",
        axum::routing::post(account_totp_start),
    )
}

pub async fn account_totp_start(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.mfa.totp.start";
    let user = challenge_user(&state, data.user_id, OP).await?;
    Ok(ok_response(start_totp(&state, &user, OP).await?))
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
