use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{certificates, hosts, ingress},
    infra::{
        database::entity::{managed_certificate, project_host_binding, regional_ingress},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/hosts/{host_id}/certificate/renew",
        axum::routing::post(renew),
    )
}

async fn binding_and_ingress(
    state: &ControlApiState,
    session: &Session,
    project_id: Uuid,
    host_id: Uuid,
    write: bool,
    op: &'static str,
) -> Result<(project_host_binding::Model, regional_ingress::Model), AppError> {
    let access = crate::domain::project_access::load(
        state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        op,
    )
    .await?;
    if write {
        access.require_member(op)?;
    }
    let db = crate::infra::http::database(state, op)?;
    let binding = load_binding(db, &access, host_id, op).await?;
    if !matches!(
        binding.kind,
        crate::infra::database::entity::HostBindingKind::Custom
    ) {
        return Err(AppError::Conflict {
            op,
            message: "this endpoint manages custom domain certificates".to_owned(),
        });
    }
    let ingress = ingress::get_enabled_by_region(db, &binding.region)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::Conflict {
            op,
            message: "no enabled regional ingress is configured".to_owned(),
        })?;
    Ok((binding, ingress))
}

fn require_eligible(
    binding: &project_host_binding::Model,
    ingress: &regional_ingress::Model,
    op: &'static str,
) -> Result<(), AppError> {
    if !certificates::binding_eligible(binding) || !ingress.enabled {
        return Err(AppError::Conflict {
            op,
            message: "verify domain ownership, complete domain review and enable the binding first"
                .to_owned(),
        });
    }
    Ok(())
}

async fn renew(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.renew";
    let (binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, true, OP).await?;
    require_eligible(&binding, &ingress, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let item = certificates::ensure_record(db, &ingress, Some(&binding))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let item = certificates::queue(db, &item)
        .await
        .map_err(|source| AppError::Conflict {
            op: OP,
            message: source.to_string(),
        })?;
    Ok(ok_response(RenewResponse {
        certificate: certificate_view(db, Some(&item), &ingress, OP).await?,
    }))
}

async fn certificate_view(
    db: &sea_orm::DatabaseConnection,
    item: Option<&managed_certificate::Model>,
    ingress: &regional_ingress::Model,
    op: &'static str,
) -> Result<CertificateResponse, AppError> {
    let issuer = crate::domain::certificate_settings::issuer(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    Ok(CertificateResponse::from_record(item, ingress, &issuer))
}

async fn load_binding(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    host_id: Uuid,
    op: &'static str,
) -> Result<project_host_binding::Model, AppError> {
    let binding = hosts::get_binding_by_id(db, host_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        })?;
    if binding.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        });
    }
    Ok(binding)
}

#[derive(Clone, serde::Serialize)]
struct CertificateResponse {
    enabled: bool,
    status: String,
    issuer: String,
    platform_issuer: String,
    challenge_method: &'static str,
    auto_renew: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    issued_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    expires_at: Option<time::OffsetDateTime>,
    error: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    retry_at: Option<time::OffsetDateTime>,
    revision: String,
}

impl CertificateResponse {
    fn from_record(
        item: Option<&crate::infra::database::entity::managed_certificate::Model>,
        ingress: &crate::infra::database::entity::regional_ingress::Model,
        issuer: &str,
    ) -> Self {
        Self {
            enabled: ingress.enabled,
            status: if !ingress.enabled || ingress.deleted_at.is_some() {
                "disabled".to_owned()
            } else {
                item.map(|item| item.status.clone())
                    .unwrap_or_else(|| "pending".to_owned())
            },
            issuer: item
                .map(|item| item.issuer.clone())
                .unwrap_or_else(|| issuer.to_owned()),
            platform_issuer: issuer.to_owned(),
            challenge_method: "http01",
            auto_renew: item.is_none_or(|item| item.auto_renew),
            issued_at: item.and_then(|item| item.issued_at),
            expires_at: item.and_then(|item| item.expires_at),
            error: item.and_then(|item| item.error.clone()),
            retry_at: item.and_then(|item| item.retry_at),
            revision: item.map(|item| item.revision.clone()).unwrap_or_default(),
        }
    }
}

#[derive(serde::Serialize)]
struct RenewResponse {
    certificate: CertificateResponse,
}
