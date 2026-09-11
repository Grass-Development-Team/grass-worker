use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, Set};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{certificates, ingress},
    infra::{
        database::entity::{managed_certificate, project_host_binding, regional_ingress},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

async fn binding_and_ingress(
    state: &ControlApiState,
    session: &Session,
    project_id: Uuid,
    host_id: Uuid,
    write: bool,
    op: &'static str,
) -> Result<(project_host_binding::Model, regional_ingress::Model), AppError> {
    let access = super::project_access(state, session, project_id, false, op).await?;
    if write {
        access.require_member(op)?;
    }
    let db = super::database(state, op)?;
    let binding = super::hosts::load_binding(db, &access, host_id, op).await?;
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

pub async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.get";
    let (_binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, false, OP).await?;
    let db = super::database(&state, OP)?;
    let item = managed_certificate::Entity::find_by_id(host_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let view = certificate_view(db, item.as_ref(), &ingress, OP).await?;
    Ok(ok_response(json!({"certificate":view})))
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
    let db = super::database(&state, OP)?;
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
    Ok(ok_response(
        json!({"certificate":certificate_view(db, Some(&item), &ingress, OP).await?}),
    ))
}

pub async fn renew(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.renew";
    let (binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, true, OP).await?;
    require_eligible(&binding, &ingress, OP)?;
    let db = super::database(&state, OP)?;
    let item = certificates::ensure_record(db, &ingress, Some(&binding))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let item = certificates::queue(db, &item)
        .await
        .map_err(|source| AppError::Conflict {
            op: OP,
            message: source.to_string(),
        })?;
    Ok(ok_response(
        json!({"certificate":certificate_view(db, Some(&item), &ingress, OP).await?}),
    ))
}

pub async fn import(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<certificates::PemBundle>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.certificate.import";
    let (binding, ingress) =
        binding_and_ingress(&state, &session, project_id, host_id, true, OP).await?;
    require_eligible(&binding, &ingress, OP)?;
    let db = super::database(&state, OP)?;
    let secret = state.config.read().unwrap().secrets.secret_key.clone();
    let item = certificates::ensure_record(db, &ingress, Some(&binding))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let item = certificates::import(db, &item, body, &secret)
        .await
        .map_err(|source| AppError::Validation {
            op: OP,
            message: source.to_string(),
        })?;
    Ok(ok_response(
        json!({"certificate":certificate_view(db, Some(&item), &ingress, OP).await?}),
    ))
}

async fn certificate_view(
    db: &sea_orm::DatabaseConnection,
    item: Option<&managed_certificate::Model>,
    ingress: &regional_ingress::Model,
    op: &'static str,
) -> Result<serde_json::Value, AppError> {
    let issuer = crate::domain::certificate_settings::issuer(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    Ok(certificates::view(item, ingress, &issuer))
}
