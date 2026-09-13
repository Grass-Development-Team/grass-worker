use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::mfa::{challenge_user, send_email_code, start_email_factor},
    infra::{
        database::entity::user_mfa_factor,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/me/mfa/email/start",
        axum::routing::post(account_email_start),
    )
}

async fn account_email_start(
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
