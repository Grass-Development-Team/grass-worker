pub(crate) mod by_factor_id;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{
        authentication::{self, UserMfaPolicy},
        users,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/users/{user_id}/mfa",
            axum::routing::get(mfa_factors).patch(update_mfa_policy),
        )
        .merge(by_factor_id::router())
}

async fn mfa_factors(
    State(state): State<ControlApiState>,
    Path(user_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.list";
    let db = crate::infra::http::database(&state, OP)?;
    let user = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let factors = authentication::mfa_factors(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let platform_policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let user_policy = authentication::user_mfa_policy(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = platform_policy.requirements_for(&user_policy, &user.platform_role);
    Ok(ok_response(MfaFactorsResponse {
        factors: factors
            .iter()
            .map(|factor| MfaFactorsFactorsResponse {
                id: factor.id,
                kind: (factor.kind.as_str()).to_owned(),
                label: factor.label.clone(),
                verified: factor.verified_at.is_some(),
                verified_at: factor.verified_at,
                last_used_at: factor.last_used_at,
                created_at: factor.created_at,
            })
            .collect::<Vec<_>>(),
        policy: user_policy,
        allowed_factors: platform_policy.allowed_factors,
        effective_requirements: MfaFactorsEffectiveRequirementsResponse {
            minimum_factors: requirements.minimum_factors,
            required_factors: requirements
                .required_factors
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>(),
        },
    }))
}

async fn update_mfa_policy(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(user_id): Path<Uuid>,
    Json(policy): Json<UserMfaPolicy>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.users.mfa.policy.update";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
    let user = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let platform_policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    policy
        .validate(&platform_policy)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    authentication::set_user_mfa_policy(db, user_id, &policy)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = platform_policy.requirements_for(&policy, &user.platform_role);
    audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "user.mfa_policy_updated".to_owned(),
            target_type: "user".to_owned(),
            target_id: Some(user_id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "inherit_platform": policy.inherit_platform,
                "minimum_factors": policy.minimum_factors,
                "required_factors": policy.required_factors,
            }),
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
    Ok(ok_response(UpdateMfaPolicyResponse {
        policy,
        effective_requirements: UpdateMfaPolicyEffectiveRequirementsResponse {
            minimum_factors: requirements.minimum_factors,
            required_factors: requirements
                .required_factors
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>(),
        },
    }))
}

#[derive(serde::Serialize)]
struct MfaFactorsFactorsResponse {
    id: uuid::Uuid,
    kind: String,
    label: Option<String>,
    verified: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    verified_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_used_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct MfaFactorsEffectiveRequirementsResponse {
    minimum_factors: usize,
    required_factors: Vec<&'static str>,
}

#[derive(serde::Serialize)]
struct MfaFactorsResponse {
    factors: Vec<MfaFactorsFactorsResponse>,
    policy: crate::domain::authentication::UserMfaPolicy,
    allowed_factors: Vec<String>,
    effective_requirements: MfaFactorsEffectiveRequirementsResponse,
}

#[derive(serde::Serialize)]
struct UpdateMfaPolicyEffectiveRequirementsResponse {
    minimum_factors: usize,
    required_factors: Vec<&'static str>,
}

#[derive(serde::Serialize)]
struct UpdateMfaPolicyResponse {
    policy: crate::domain::authentication::UserMfaPolicy,
    effective_requirements: UpdateMfaPolicyEffectiveRequirementsResponse,
}
