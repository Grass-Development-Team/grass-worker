pub(crate) mod by_slug;

use axum::{extract::State, response::IntoResponse};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::{
    domain::{authentication, registration, settings},
    infra::{
        database::entity::auth_identity_provider,
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/providers", axum::routing::get(providers))
        .merge(by_slug::router())
}

async fn providers(State(state): State<ControlApiState>) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "auth.providers.list";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let providers = auth_identity_provider::Entity::find()
        .filter(auth_identity_provider::Column::Enabled.eq(true))
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let signup_policy = settings::get_setting(db, "signup.policy")
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let signup_policy = registration::SignupPolicy::parse(
        signup_policy
            .as_ref()
            .and_then(|setting| setting.value.as_str()),
    )
    .map_err(|error| AppError::Internal {
        op: OP,
        message: error.to_string(),
    })?;
    let password_recovery_available = state.config.read().unwrap().mail.enabled();
    Ok(ok_response(ProvidersResponse {
        providers: providers
            .iter()
            .map(|provider| ProvidersProvidersResponse {
                slug: provider.slug.clone(),
                name: provider.name.clone(),
                kind: (provider.kind.as_str()).to_owned(),
            })
            .collect::<Vec<_>>(),
        password_recovery_available,
        signup_policy: (signup_policy.as_str()).to_owned(),
        registration_email_verification: authentication::registration_verification_required(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
        password_policy: authentication::password_policy(db)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?,
    }))
}

#[derive(serde::Serialize)]
struct ProvidersProvidersResponse {
    slug: String,
    name: String,
    kind: String,
}

#[derive(serde::Serialize)]
struct ProvidersResponse {
    providers: Vec<ProvidersProvidersResponse>,
    password_recovery_available: bool,
    signup_policy: String,
    registration_email_verification: bool,
    password_policy: crate::domain::authentication::PasswordPolicy,
}
