pub(crate) mod import;
pub(crate) mod renew;

use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
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
    axum::Router::new()
        .route(
            "/projects/{project_id}/hosts/{host_id}/certificate",
            axum::routing::get(get).patch(update),
        )
        .merge(import::router())
        .merge(renew::router())
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

pub async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.get";
    let (_binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, false, OP).await?;
    let db = crate::infra::http::database(&state, OP)?;
    let item = managed_certificate::Entity::find_by_id(host_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let view = certificate_view(db, item.as_ref(), &ingress, OP).await?;
    Ok(ok_response(GetResponse { certificate: view }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    pub challenge_method: Option<String>,
    pub certificate_auto_renew: Option<bool>,
    pub certificate_issuer: Option<String>,
}

pub async fn update(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.update";
    let (binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, true, OP).await?;
    let db = crate::infra::http::database(&state, OP)?;
    let item = certificates::ensure_record(db, &ingress, Some(&binding))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let mut active: managed_certificate::ActiveModel = item.clone().into();
    let mut restart = false;
    if let Some(method) = body.challenge_method {
        if method != "http01" {
            return Err(AppError::Validation {
                op: OP,
                message: "Custom domains use HTTP validation (http01)".to_owned(),
            });
        }
        restart |= method != item.challenge_method;
        active.challenge_method = Set(method);
    }
    if let Some(value) = body.certificate_auto_renew {
        active.auto_renew = Set(value);
    }
    if let Some(issuer) = body.certificate_issuer {
        if issuer != "manual"
            && issuer
                != crate::domain::certificate_settings::issuer(db)
                    .await
                    .map_err(|source| AppError::Infrastructure { op: OP, source })?
        {
            return Err(AppError::Validation {
                op: OP,
                message: "automatic issuer must match the platform certificate authority"
                    .to_owned(),
            });
        }
        restart |= issuer != item.issuer;
        active.issuer = Set(issuer);
        active.acme_account = Set(None);
    }
    if restart {
        if item
            .lease_until
            .is_some_and(|until| until > time::OffsetDateTime::now_utc())
        {
            return Err(AppError::Conflict {
                op: OP,
                message: "wait for the in-progress certificate attempt before changing its method"
                    .to_owned(),
            });
        }
        active.generation = Set(Uuid::now_v7());
        active.status = Set("pending".to_owned());
        active.retry_at = Set(None);
        active.failure_count = Set(0);
    }
    active.generation = Set(Uuid::now_v7());
    let item = managed_certificate::Entity::update(active)
        .validate()
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .filter(managed_certificate::Column::Generation.eq(item.generation))
        .filter(
            Condition::any()
                .add(managed_certificate::Column::LeaseUntil.is_null())
                .add(managed_certificate::Column::LeaseUntil.lte(time::OffsetDateTime::now_utc())),
        )
        .exec(db)
        .await
        .map_err(|source| match source {
            sea_orm::DbErr::RecordNotUpdated => AppError::Conflict {
                op: OP,
                message: "Certificate changed or issuance is in progress. Refresh and try again."
                    .to_owned(),
            },
            source => AppError::Infrastructure {
                op: OP,
                source: source.into(),
            },
        })?;
    Ok(ok_response(UpdateResponse {
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

pub(super) async fn load_binding(
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
struct GetResponse {
    certificate: CertificateResponse,
}

#[derive(serde::Serialize)]
struct UpdateResponse {
    certificate: CertificateResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_response_disables_the_entry_and_omits_private_material() {
        let mut ingress = crate::domain::certificates::tests::ingress_fixture();
        let mut certificate = crate::domain::certificates::tests::certificate_fixture(&ingress);
        certificate.contact_email = "private-contact@example.test".to_owned();
        certificate.bundle = Some(serde_json::json!({"private_key": "private-certificate-key"}));
        ingress.enabled = false;
        let value = serde_json::to_value(CertificateResponse::from_record(
            Some(&certificate),
            &ingress,
            "letsencrypt",
        ))
        .unwrap();
        assert_eq!(value["status"], "disabled");
        assert_eq!(value["platform_issuer"], "letsencrypt");
        assert_eq!(value["challenge_method"], "http01");
        assert!(!value.to_string().contains("private-contact"));
        assert!(!value.to_string().contains("private-certificate-key"));
        assert!(!value.as_object().unwrap().contains_key("installed_nodes"));
    }
}
