use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::authentication,
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/users/{user_id}/mfa/{factor_id}",
        axum::routing::delete(reset_mfa_factor),
    )
}

pub async fn reset_mfa_factor(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path((user_id, factor_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.reset";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let factor = authentication::mfa_factor(db, user_id, factor_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "MFA factor not found".to_owned(),
        })?;
    authentication::delete_mfa_factor(db, user_id, factor_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.mfa_factor_reset".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(user_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "factor_id": factor.id, "factor_kind": factor.kind.as_str() }),
        },
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
    Ok(ok_response(ResetMfaFactorResponse { deleted: true }))
}

#[derive(serde::Serialize)]
struct ResetMfaFactorResponse {
    deleted: bool,
}
