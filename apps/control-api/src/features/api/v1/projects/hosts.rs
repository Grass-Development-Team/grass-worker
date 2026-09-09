use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ActiveModelTrait, EntityTrait, TransactionTrait};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::hosts::{self, DomainReviewMode},
    domain::{certificates, deployments, ingress},
    infra::{
        database::entity::{
            DeploymentEnvironment, HostBindingEnvironment, HostBindingKind, HostBindingStatus,
            HostReviewStatus, host_policy, project_host_binding,
        },
        error::{AppError, ok_response},
        host_provision::service::{BindHostRequest, HostBindingService},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn binding_view(binding: &project_host_binding::Model) -> serde_json::Value {
    json!({
        "id": binding.id,
        "project_id": binding.project_id,
        "host": binding.host,
        "region": binding.region,
        "kind": match binding.kind {
            HostBindingKind::Platform => "platform",
            HostBindingKind::Custom => "custom",
        },
        "environment": match binding.environment {
            HostBindingEnvironment::Production => "production",
            HostBindingEnvironment::Preview => "preview",
            HostBindingEnvironment::All => "all",
        },
        "status": status_value(&binding.status),
        "failure_reason": binding.failure_reason,
        "is_primary": binding.is_primary,
        "host_source_id": binding.host_source_id,
        "review_status": review_status_value(&binding.review_status),
        "reviewed_by_user_id": binding.reviewed_by_user_id,
        "reviewed_at": binding.reviewed_at.map(ts),
        "review_reason": binding.review_reason,
        "ownership_status": binding.ownership_status,
        "ownership_checked_at": binding.ownership_checked_at.map(ts),
        "ownership_error": binding.ownership_error,
        "created_at": ts(binding.created_at),
        "ingress": serde_json::Value::Null,
    })
}

async fn attach_ingress_guidance(
    state: &ControlApiState,
    db: &sea_orm::DatabaseConnection,
    binding: &project_host_binding::Model,
    mut view: serde_json::Value,
    op: &'static str,
) -> Result<serde_json::Value, AppError> {
    if !matches!(binding.kind, HostBindingKind::Custom) {
        return Ok(view);
    }
    let Some(regional_ingress) = ingress::get_enabled_by_region(db, &binding.region)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    else {
        return Ok(view);
    };
    let candidates = ingress::healthy_serve_nodes(db, &binding.region, OffsetDateTime::now_utc())
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
    let verification_value =
        ingress::dns_verification_token(&secret_key, binding.id, &binding.host);
    let guidance = ingress::cname_guidance(ingress::CnameGuidanceInput {
        host: &binding.host,
        region: &binding.region,
        ingress_hostname: &regional_ingress.hostname,
        verification_name: &format!("_grass.{}", binding.host),
        verification_value: &verification_value,
        origin_host_preservation: regional_ingress.origin_host_preservation,
    });
    view["ingress"] = json!({
        "region": guidance.region,
        "cname": {
            "record_type": guidance.record_type,
            "name": guidance.name,
            "target": guidance.target,
        },
        "txt": {
            "record_type": "TXT",
            "name": guidance.verification_name,
            "value": guidance.verification_value,
        },
        "origin_host_preservation": guidance.origin_host_preservation,
        "health_check": {
            "path": regional_ingress.health_check_path,
            "interval_seconds": regional_ingress.health_check_interval_seconds,
        },
        "entrance_nodes": candidates.iter().map(|candidate| json!({
            "node_id": candidate.node_id,
            "base_url": candidate.base_url,
            "priority": candidate.priority,
        })).collect::<Vec<_>>(),
        "certificate": {
            "enabled": regional_ingress.tls_enabled,
            "issuer": regional_ingress.certificate_issuer,
            "auto_renew": regional_ingress.certificate_auto_renew,
            "status": regional_ingress.certificate_status,
            "expires_at": ts(regional_ingress.certificate_expires_at),
            "error": regional_ingress.certificate_error,
        },
        "dns_challenge": {
            "provider": regional_ingress.dns_challenge_provider,
            "status": regional_ingress.dns_challenge_status,
            "record_name": regional_ingress.dns_challenge_record_name,
            "record_value": regional_ingress.dns_challenge_record_value,
        },
    });
    let record =
        crate::infra::database::entity::managed_certificate::Entity::find_by_id(binding.id)
            .one(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;
    let mut certificate = certificates::view(record.as_ref(), &regional_ingress);
    certificate["dns_delegation_name"] = json!(format!("_acme-challenge.{}", binding.host));
    certificate["dns_delegation_target"] = json!(format!(
        "_acme-{}.{}",
        binding.id.simple(),
        regional_ingress.hostname
    ));
    view["certificate"] = certificate.clone();
    view["ingress"]["certificate"] = certificate;
    Ok(view)
}

fn review_status_value(status: &HostReviewStatus) -> &'static str {
    match status {
        HostReviewStatus::NotRequired => "not_required",
        HostReviewStatus::Pending => "pending",
        HostReviewStatus::Approved => "approved",
        HostReviewStatus::Rejected => "rejected",
    }
}

fn status_value(status: &HostBindingStatus) -> &'static str {
    match status {
        HostBindingStatus::Pending => "pending",
        HostBindingStatus::Active => "active",
        HostBindingStatus::Failed => "failed",
        HostBindingStatus::Disabled => "disabled",
    }
}

fn parse_environment(value: &str, op: &'static str) -> Result<HostBindingEnvironment, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" => Ok(HostBindingEnvironment::Production),
        "preview" => Ok(HostBindingEnvironment::Preview),
        "all" => Ok(HostBindingEnvironment::All),
        other => Err(AppError::Validation {
            op,
            message: format!("invalid environment: {other}"),
        }),
    }
}

/// GET /api/v1/projects/{project_id}/hosts
pub async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.list";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;

    let bindings = hosts::list_bindings_for_project(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let production_deployment =
        deployments::find_active(db, access.project.id, DeploymentEnvironment::Production)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_deployment =
        deployments::find_active(db, access.project.id, DeploymentEnvironment::Preview)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let mut views = Vec::with_capacity(bindings.len());
    for binding in &bindings {
        let events = hosts::list_provision_events_for_binding(db, binding.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let view = binding_view(binding);
        let mut view = attach_ingress_guidance(&state, db, binding, view, OP).await?;
        view["serving"] = json!(
            matches!(binding.status, HostBindingStatus::Active)
                && match binding.environment {
                    HostBindingEnvironment::Production => production_deployment.is_some(),
                    HostBindingEnvironment::Preview => preview_deployment.is_some(),
                    HostBindingEnvironment::All => {
                        production_deployment.is_some() || preview_deployment.is_some()
                    }
                }
        );
        view["provision_events"] = json!(
            events
                .iter()
                .take(10)
                .map(|event| json!({
                    "id": event.id,
                    "status": match event.status {
                        crate::infra::database::entity::HostProvisionEventStatus::Success => "success",
                        crate::infra::database::entity::HostProvisionEventStatus::Pending => "pending",
                        crate::infra::database::entity::HostProvisionEventStatus::Failed => "failed",
                    },
                    "operation": event.operation,
                    "error_message": event.error_message,
                    "created_at": ts(event.created_at),
                }))
                .collect::<Vec<_>>()
        );
        views.push(view);
    }

    Ok(ok_response(json!({ "hosts": views })))
}

#[derive(Deserialize)]
pub struct CreateHostRequest {
    pub host: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default = "default_environment")]
    pub environment: String,
    #[serde(default)]
    pub host_source_id: Option<Uuid>,
}

fn default_environment() -> String {
    "production".to_owned()
}

/// POST /api/v1/projects/{project_id}/hosts
pub async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateHostRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.create";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let environment = parse_environment(&body.environment, OP)?;
    let source = match body.host_source_id {
        Some(source_id) => Some(
            hosts::get_source_by_id(db, source_id)
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?
                .ok_or_else(|| AppError::NotFound {
                    op: OP,
                    message: "host source not found".to_owned(),
                })?,
        ),
        None => None,
    };
    let region = body
        .region
        .as_deref()
        .map(grass_validator::normalize_region)
        .transpose()
        .map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?
        .or_else(|| source.as_ref().map(|source| source.region.clone()))
        .unwrap_or_else(|| "default".to_owned());

    if let Some(source) = source.as_ref()
        && region != source.region
    {
        return Err(AppError::Validation {
            op: OP,
            message: format!(
                "region must match the host source region ({})",
                source.region
            ),
        });
    }

    // Custom hosts require the team group policy to allow them; hosts under
    // a platform source must live under that source's base domain.
    let policy = hosts::policy_for_team_group(db, access.team.group_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    match &source {
        Some(source) => {
            if !host.ends_with(&format!(".{}", source.base_domain)) {
                return Err(AppError::Validation {
                    op: OP,
                    message: format!(
                        "host must be a subdomain of {} for this host source",
                        source.base_domain
                    ),
                });
            }
        }
        None => {
            if let Some(host_policy::Model {
                allow_custom_hosts: false,
                ..
            }) = policy
            {
                return Err(AppError::Forbidden {
                    op: OP,
                    message: "team group does not allow custom hosts".to_owned(),
                });
            }
        }
    }

    let existing = hosts::list_bindings_for_project(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let is_primary = existing.is_empty();
    let review_status = if source.is_some() {
        HostReviewStatus::NotRequired
    } else {
        match hosts::domain_review_policy_for_team(db, access.team.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
        {
            DomainReviewMode::Auto => HostReviewStatus::Approved,
            DomainReviewMode::Manual => HostReviewStatus::Pending,
        }
    };

    let service = HostBindingService::new(db, cache);
    let binding = service
        .bind_host(
            OP,
            BindHostRequest {
                project: &access.project,
                team: &access.team,
                source: source.as_ref(),
                host,
                region,
                kind: if source.is_some() {
                    HostBindingKind::Platform
                } else {
                    HostBindingKind::Custom
                },
                environment,
                is_primary,
                review_status,
                actor_user_id: Some(session.data.user_id),
            },
        )
        .await?;

    let view = attach_ingress_guidance(&state, db, &binding, binding_view(&binding), OP).await?;
    Ok(ok_response(json!({ "host": view })))
}

#[derive(Deserialize)]
pub struct UpdateHostRequest {
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// POST /api/v1/projects/{project_id}/hosts/{host_id}/verify
pub async fn verify(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.verify";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let binding = load_binding(db, &access, host_id, OP).await?;
    if !matches!(binding.kind, HostBindingKind::Custom) {
        return Err(AppError::Conflict {
            op: OP,
            message: "platform domains do not require ownership verification".to_owned(),
        });
    }
    let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
    let expected = ingress::dns_verification_token(&secret_key, binding.id, &binding.host);
    let result = ingress::verify_dns_txt(&binding.host, &expected)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let binding = hosts::get_binding_by_id_for_update(&transaction, host_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .filter(|binding| binding.project_id == project_id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host binding no longer exists".to_owned(),
        })?;
    let mut active: project_host_binding::ActiveModel = binding.clone().into();
    active.ownership_checked_at = sea_orm::ActiveValue::Set(Some(OffsetDateTime::now_utc()));
    match result {
        ingress::DnsVerification::Verified => {
            active.ownership_status = sea_orm::ActiveValue::Set("verified".to_owned());
            active.ownership_error = sea_orm::ActiveValue::Set(None);
            if matches!(binding.review_status, HostReviewStatus::Approved) {
                active.status = sea_orm::ActiveValue::Set(HostBindingStatus::Active);
            }
        }
        ingress::DnsVerification::Missing => {
            active.ownership_status = sea_orm::ActiveValue::Set("failed".to_owned());
            active.ownership_error =
                sea_orm::ActiveValue::Set(Some("TXT ownership record was not found".to_owned()));
            active.status = sea_orm::ActiveValue::Set(HostBindingStatus::Pending);
        }
        ingress::DnsVerification::Mismatch => {
            active.ownership_status = sea_orm::ActiveValue::Set("failed".to_owned());
            active.ownership_error =
                sea_orm::ActiveValue::Set(Some("TXT ownership record did not match".to_owned()));
            active.status = sea_orm::ActiveValue::Set(HostBindingStatus::Pending);
        }
    }
    if matches!(binding.status, HostBindingStatus::Disabled) {
        active.status = sea_orm::ActiveValue::Set(HostBindingStatus::Disabled);
    }
    let updated = active
        .update(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let view = attach_ingress_guidance(&state, db, &updated, binding_view(&updated), OP).await?;
    Ok(ok_response(
        json!({ "host": view, "verified": result == ingress::DnsVerification::Verified }),
    ))
}

/// PATCH /api/v1/projects/{project_id}/hosts/{host_id}
pub async fn update(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateHostRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.update";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;

    let binding = load_binding(db, &access, host_id, OP).await?;

    let mut active: project_host_binding::ActiveModel = binding.into();
    if let Some(environment) = body.environment {
        active.environment = sea_orm::ActiveValue::Set(parse_environment(&environment, OP)?);
    }
    if let Some(status) = body.status {
        // Only enabling and disabling are user-controlled; provision status
        // transitions come from provisioning.
        let status = match status.trim().to_ascii_lowercase().as_str() {
            "disabled" => HostBindingStatus::Disabled,
            "pending" => HostBindingStatus::Pending,
            other => {
                return Err(AppError::Validation {
                    op: OP,
                    message: format!("status can only be set to disabled or pending, not {other}"),
                });
            }
        };
        active.status = sea_orm::ActiveValue::Set(status);
    }

    let binding = sea_orm::ActiveModelTrait::update(active, db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let view = attach_ingress_guidance(&state, db, &binding, binding_view(&binding), OP).await?;
    Ok(ok_response(json!({ "host": view })))
}

/// DELETE /api/v1/projects/{project_id}/hosts/{host_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.remove";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let binding = hosts::get_binding_by_id_for_update_including_deleted(&transaction, host_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .filter(|binding| binding.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host binding not found".to_owned(),
        })?;
    let binding_id = binding.id;
    if binding.deleted_at.is_none() {
        hosts::soft_delete_binding(&transaction, binding.clone())
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    if let Some(source_id) = binding.host_source_id
        && let Some(source) = hosts::get_source_by_id(db, source_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        let _ = HostBindingService::new(db, cache)
            .deprovision(OP, &binding, &source)
            .await?;
    }

    crate::infra::quota::QuotaService::new(db, cache)
        .release_once(
            OP,
            access.team.id,
            &[crate::infra::quota::QuotaCharge::one(
                crate::domain::quotas::QuotaDimension::Hosts,
            )],
            "project_host_binding",
            binding_id,
        )
        .await?;

    Ok(ok_response(json!({ "ok": true })))
}

/// POST /api/v1/projects/{project_id}/hosts/{host_id}/primary
pub async fn set_primary(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.set_primary";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;

    let binding = load_binding(db, &access, host_id, OP).await?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    hosts::set_primary_binding(&transaction, access.project.id, binding.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(json!({ "ok": true })))
}

/// POST /api/v1/projects/{project_id}/hosts/{host_id}/provision — retry
/// provisioning for pending or failed bindings.
pub async fn provision(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.provision";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

    let binding = load_binding(db, &access, host_id, OP).await?;
    let Some(source_id) = binding.host_source_id else {
        return Err(AppError::Validation {
            op: OP,
            message: "custom hosts have no provisioner to run".to_owned(),
        });
    };
    let source = hosts::get_source_by_id(db, source_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host source not found".to_owned(),
        })?;

    let service = HostBindingService::new(db, cache);
    let binding = service.provision(OP, binding, &source).await?;

    let view = attach_ingress_guidance(&state, db, &binding, binding_view(&binding), OP).await?;
    Ok(ok_response(json!({ "host": view })))
}

pub(super) async fn load_binding(
    db: &sea_orm::DatabaseConnection,
    access: &super::ProjectAccess,
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

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn host_view_exposes_review_state_separately_from_provisioning() {
        let reviewer_id = Uuid::now_v7();
        let binding = project_host_binding::Model {
            id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            host_source_id: None,
            host: "manual.example.test".to_owned(),
            region: "default".to_owned(),
            kind: HostBindingKind::Custom,
            environment: HostBindingEnvironment::Production,
            status: HostBindingStatus::Pending,
            failure_reason: None,
            is_primary: false,
            review_status: HostReviewStatus::Rejected,
            reviewed_by_user_id: Some(reviewer_id),
            reviewed_at: Some(OffsetDateTime::UNIX_EPOCH),
            review_reason: Some("Ownership could not be verified".to_owned()),
            ownership_status: "pending".to_owned(),
            ownership_checked_at: None,
            ownership_error: None,
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };

        let view = binding_view(&binding);

        assert_eq!(view["status"], "pending");
        assert_eq!(view["review_status"], "rejected");
        assert_eq!(view["reviewed_by_user_id"], reviewer_id.to_string());
        assert!(view["reviewed_at"].is_string());
        assert_eq!(view["review_reason"], "Ownership could not be verified");
    }
}
