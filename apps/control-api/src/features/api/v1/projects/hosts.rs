use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::hosts::{self, DomainReviewMode},
    infra::{
        database::entity::{
            HostBindingEnvironment, HostBindingKind, HostBindingStatus, HostReviewStatus,
            host_policy, project_host_binding,
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
        "created_at": ts(binding.created_at),
    })
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

    let mut views = Vec::with_capacity(bindings.len());
    for binding in &bindings {
        let events = hosts::list_provision_events_for_binding(db, binding.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let mut view = binding_view(binding);
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

    Ok(ok_response(json!({ "host": binding_view(&binding) })))
}

#[derive(Deserialize)]
pub struct UpdateHostRequest {
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
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

    Ok(ok_response(json!({ "host": binding_view(&binding) })))
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
        HostBindingService::new(db, cache)
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

    Ok(ok_response(json!({ "host": binding_view(&binding) })))
}

async fn load_binding(
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
            kind: HostBindingKind::Custom,
            environment: HostBindingEnvironment::Production,
            status: HostBindingStatus::Pending,
            failure_reason: None,
            is_primary: false,
            review_status: HostReviewStatus::Rejected,
            reviewed_by_user_id: Some(reviewer_id),
            reviewed_at: Some(OffsetDateTime::UNIX_EPOCH),
            review_reason: Some("Ownership could not be verified".to_owned()),
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
