use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::mfa::{challenge_user, start_totp},
    infra::{
        database::entity::user_mfa_factor,
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
    let enrollment = start_totp(&state, &user, OP).await?;
    Ok(ok_response(TotpEnrollmentResponse {
        factor: factor_view(&enrollment.factor),
        secret: enrollment.secret,
        otpauth_uri: enrollment.otpauth_uri,
    }))
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
