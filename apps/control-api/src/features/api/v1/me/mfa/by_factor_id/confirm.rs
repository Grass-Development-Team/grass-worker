use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        authentication,
        mfa::{enforce_attempt_limit, factor_for_user, record_factor_audit, verify_factor_code},
    },
    infra::{
        database::entity::user_mfa_factor,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/me/mfa/{factor_id}/confirm",
        axum::routing::post(account_confirm),
    )
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
