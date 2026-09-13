use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::{authentication, users},
    infra::{
        database::entity::{MfaFactorKind, user_mfa_factor},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/me/security", axum::routing::get(security))
}

async fn security(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.security";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let user = users::get_user_by_id(db, data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "user not found".to_owned(),
        })?;
    let policy = authentication::mfa_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let factors = authentication::verified_mfa_factors(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let password_policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let user_policy = authentication::user_mfa_policy(db, user.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let requirements = policy.requirements_for(&user_policy, &user.platform_role);
    Ok(ok_response(SecurityResponse {
        email_verified: user.email_verified_at.is_some(),
        factors: factors.iter().map(factor_view).collect::<Vec<_>>(),
        allowed_factors: policy.allowed_factors,
        mfa_required: requirements.is_enforced(),
        mfa_requirements: SecurityMfaRequirementsResponse {
            minimum_factors: requirements.minimum_factors,
            required_factors: requirements
                .required_factors
                .iter()
                .map(MfaFactorKind::as_str)
                .collect::<Vec<_>>(),
        },
        mfa_policy: user_policy,
        password_policy,
        mail_available: state.config.read().unwrap().mail.enabled(),
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
struct SecurityMfaRequirementsResponse {
    minimum_factors: usize,
    required_factors: Vec<&'static str>,
}

#[derive(serde::Serialize)]
struct SecurityResponse {
    email_verified: bool,
    factors: Vec<MfaFactorResponse>,
    allowed_factors: Vec<String>,
    mfa_required: bool,
    mfa_requirements: SecurityMfaRequirementsResponse,
    mfa_policy: crate::domain::authentication::UserMfaPolicy,
    password_policy: crate::domain::authentication::PasswordPolicy,
    mail_available: bool,
}
