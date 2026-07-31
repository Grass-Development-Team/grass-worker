use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QuerySelect, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        hosts, notifications, projects,
    },
    infra::{
        database::entity::{
            AuditEventResult, DeploymentReleaseStatus, HostBindingKind, HostBindingStatus,
            HostReviewStatus, deployment, project_host_binding,
        },
        error::{AppError, ok_response},
        host_provision::service::HostBindingService,
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
        route_invalidation,
    },
    state::ControlApiState,
};

#[derive(Default, Deserialize)]
pub struct DomainReason {
    #[serde(default)]
    pub reason: Option<String>,
}

fn optional_reason(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn ensure_custom(binding: &project_host_binding::Model, op: &'static str) -> Result<(), AppError> {
    if !matches!(binding.kind, HostBindingKind::Custom)
        || matches!(binding.review_status, HostReviewStatus::NotRequired)
    {
        return Err(AppError::Conflict {
            op,
            message: "platform domains do not support review decisions".to_owned(),
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DomainDecision {
    Apply {
        review_status: HostReviewStatus,
        binding_status: HostBindingStatus,
    },
    Conflict,
}

fn domain_decision(current: &HostReviewStatus, approved: bool) -> DomainDecision {
    if (matches!(current, HostReviewStatus::Approved) && approved)
        || (matches!(current, HostReviewStatus::Rejected) && !approved)
    {
        DomainDecision::Conflict
    } else {
        DomainDecision::Apply {
            review_status: if approved {
                HostReviewStatus::Approved
            } else {
                HostReviewStatus::Rejected
            },
            binding_status: if approved {
                HostBindingStatus::Active
            } else {
                HostBindingStatus::Disabled
            },
        }
    }
}

async fn load<C: ConnectionTrait>(
    db: &C,
    id: Uuid,
    op: &'static str,
) -> Result<project_host_binding::Model, AppError> {
    hosts::get_binding_by_id_for_update(db, id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "domain binding not found".to_owned(),
        })
}

/// POST /api/v1/admin/domains/{domain_id}/approve
pub async fn approve(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(domain_id): Path<Uuid>,
    body: Option<Json<DomainReason>>,
) -> Result<impl IntoResponse, AppError> {
    decide(
        state,
        data.user_id,
        domain_id,
        true,
        body.and_then(|Json(body)| body.reason),
    )
    .await
}

/// POST /api/v1/admin/domains/{domain_id}/reject
pub async fn reject(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(domain_id): Path<Uuid>,
    body: Option<Json<DomainReason>>,
) -> Result<impl IntoResponse, AppError> {
    decide(
        state,
        data.user_id,
        domain_id,
        false,
        body.and_then(|Json(body)| body.reason),
    )
    .await
}

async fn decide(
    state: ControlApiState,
    actor: Uuid,
    domain_id: Uuid,
    approved: bool,
    reason: Option<String>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domains.decide";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let binding = load(&transaction, domain_id, OP).await?;
    ensure_custom(&binding, OP)?;
    let reason = optional_reason(reason);
    let DomainDecision::Apply {
        review_status: new_review_status,
        binding_status: new_binding_status,
    } = domain_decision(&binding.review_status, approved)
    else {
        return Err(AppError::Conflict {
            op: OP,
            message: "domain review has already been decided".to_owned(),
        });
    };
    let before = json!({ "review_status": review_status(&binding.review_status), "status": binding_status(&binding.status) });
    let mut active: project_host_binding::ActiveModel = binding.clone().into();
    active.review_status = Set(new_review_status);
    active.reviewed_by_user_id = Set(Some(actor));
    active.reviewed_at = Set(Some(time::OffsetDateTime::now_utc()));
    active.review_reason = Set(reason.clone());
    active.status = Set(new_binding_status);
    let updated = active
        .update(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let action = if approved {
        "domain.approved"
    } else {
        "domain.rejected"
    };
    audits::create_platform_audit_event_with_changes(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(actor), actor_node_id: None, team_id: Some(updated.team_id),
            action: action.to_owned(),
            target_type: "project_host_binding".to_owned(), target_id: Some(updated.id),
            result: AuditEventResult::Success, reason: reason.clone(), metadata: json!({ "platform_admin": true, "project_id": updated.project_id }),
        },
        json!({ "before": before, "after": { "review_status": review_status(&updated.review_status), "status": binding_status(&updated.status) } }),
    ).await.map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let project = projects::get_by_id_any(&transaction, updated.project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "project not found".to_owned(),
        })?;
    notifications::create_project_notification(
        &transaction,
        notifications::CreateProjectNotification {
            project: &project,
            actor_user_id: actor,
            action,
            reason: reason.clone(),
            target_url: format!("/projects/{}/domains", project.id),
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
    invalidate_project_routes(&state, updated.project_id, OP).await?;
    Ok(ok_response(
        json!({ "domain": domain_view(&updated), "reason": reason }),
    ))
}

/// DELETE /api/v1/admin/domains/{domain_id}
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(domain_id): Path<Uuid>,
    body: Option<Json<DomainReason>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.domains.delete";
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let reason = optional_reason(body.and_then(|Json(body)| body.reason));
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let binding = hosts::get_binding_by_id_for_update_including_deleted(&transaction, domain_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "domain binding not found".to_owned(),
        })?;
    if binding.deleted_at.is_none() {
        hosts::soft_delete_binding(&transaction, binding.clone())
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        audits::create_platform_audit_event_with_changes(
            &transaction,
            CreateAuditEventParams {
                actor_user_id: Some(data.user_id), actor_node_id: None, team_id: Some(binding.team_id),
                action: "domain.deleted".to_owned(), target_type: "project_host_binding".to_owned(), target_id: Some(binding.id),
                result: AuditEventResult::Success, reason: reason.clone(), metadata: json!({ "platform_admin": true, "project_id": binding.project_id }),
            },
            json!({ "before": { "host": binding.host, "deleted": false }, "after": { "deleted": true } }),
        ).await.map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let project = projects::get_by_id_any(&transaction, binding.project_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
            .ok_or_else(|| AppError::NotFound {
                op: OP,
                message: "project not found".to_owned(),
            })?;
        notifications::create_project_notification(
            &transaction,
            notifications::CreateProjectNotification {
                project: &project,
                actor_user_id: data.user_id,
                action: "domain.deleted",
                reason: reason.clone(),
                target_url: format!("/projects/{}/domains", project.id),
            },
        )
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
    QuotaService::new(db, cache)
        .release_once(
            OP,
            binding.team_id,
            &[QuotaCharge::one(
                crate::domain::quotas::QuotaDimension::Hosts,
            )],
            "project_host_binding",
            binding.id,
        )
        .await?;
    invalidate_project_routes(&state, binding.project_id, OP).await?;
    Ok(ok_response(json!({ "deleted": true, "reason": reason })))
}

async fn invalidate_project_routes(
    state: &ControlApiState,
    project_id: Uuid,
    op: &'static str,
) -> Result<(), AppError> {
    let db = super::database(state, op)?;
    let deployment_ids = deployment::Entity::find()
        .select_only()
        .column(deployment::Column::Id)
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::ReleaseStatus.eq(DeploymentReleaseStatus::Active))
        .filter(deployment::Column::DeletedAt.is_null())
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
    for deployment_id in deployment_ids {
        route_invalidation::invalidate_deployment(db, &secret_key, deployment_id)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
    }
    Ok(())
}

fn review_status(value: &HostReviewStatus) -> &'static str {
    match value {
        HostReviewStatus::NotRequired => "not_required",
        HostReviewStatus::Pending => "pending",
        HostReviewStatus::Approved => "approved",
        HostReviewStatus::Rejected => "rejected",
    }
}
fn binding_status(value: &HostBindingStatus) -> &'static str {
    match value {
        HostBindingStatus::Pending => "pending",
        HostBindingStatus::Active => "active",
        HostBindingStatus::Failed => "failed",
        HostBindingStatus::Disabled => "disabled",
    }
}
fn domain_view(binding: &project_host_binding::Model) -> serde_json::Value {
    json!({ "id": binding.id, "project_id": binding.project_id, "host": binding.host, "status": binding_status(&binding.status), "review_status": review_status(&binding.review_status), "reviewed_by_user_id": binding.reviewed_by_user_id, "reviewed_at": binding.reviewed_at.map(crate::infra::http::timestamps::ts), "review_reason": binding.review_reason, "is_primary": binding.is_primary })
}

#[cfg(test)]
mod tests {
    use crate::infra::database::entity::{HostBindingStatus, HostReviewStatus};

    use super::{DomainDecision, domain_decision, optional_reason};

    #[test]
    fn reasons_trim_and_blank_is_none() {
        assert_eq!(
            optional_reason(Some("  policy  ".to_owned())),
            Some("policy".to_owned())
        );
        assert_eq!(optional_reason(Some("  ".to_owned())), None);
    }

    #[test]
    fn domain_review_decisions_drive_serving_status_without_duplicate_decisions() {
        assert_eq!(
            domain_decision(&HostReviewStatus::Pending, true),
            DomainDecision::Apply {
                review_status: HostReviewStatus::Approved,
                binding_status: HostBindingStatus::Active,
            }
        );
        assert_eq!(
            domain_decision(&HostReviewStatus::Approved, false),
            DomainDecision::Apply {
                review_status: HostReviewStatus::Rejected,
                binding_status: HostBindingStatus::Disabled,
            }
        );
        assert_eq!(
            domain_decision(&HostReviewStatus::Approved, true),
            DomainDecision::Conflict
        );
        assert_eq!(
            domain_decision(&HostReviewStatus::Rejected, false),
            DomainDecision::Conflict
        );
    }
}
