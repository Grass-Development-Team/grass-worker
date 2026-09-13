use crate::domain::mfa::{challenge_user, factor_for_user, record_factor_audit};
pub(crate) mod confirm;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::authentication,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/me/mfa/{factor_id}", axum::routing::delete(account_delete))
        .merge(confirm::router())
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
    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    authentication::delete_mfa_factor(&transaction, user.id, factor.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_factor_audit(&transaction, user.id, "mfa.factor_removed", &factor.kind)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(AccountDeleteResponse { deleted: true }))
}

#[derive(serde::Serialize)]
struct AccountDeleteResponse {
    deleted: bool,
}
