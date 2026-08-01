use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        delivery::{self, PublicationRemovalKind},
        deployments::{self, DeploymentStateError, ReviewMode},
        notifications, projects,
    },
    infra::{
        database::entity::{
            AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentReleaseStatus,
            DeploymentServeStatus, ReleaseReason, deployment,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
        route_invalidation,
    },
    state::ControlApiState,
};

#[derive(Default, Deserialize)]
pub struct GovernanceReason {
    #[serde(default)]
    pub reason: Option<String>,
}

fn optional_reason(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepublishPolicy {
    Manual,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RepublishAction {
    Release,
    Review,
    Conflict,
}

pub(crate) fn republish_action(
    status: &DeploymentReleaseStatus,
    policy: RepublishPolicy,
) -> RepublishAction {
    match (status, policy) {
        (DeploymentReleaseStatus::Approved, _)
        | (DeploymentReleaseStatus::Draft, RepublishPolicy::Auto)
        | (DeploymentReleaseStatus::Rejected, RepublishPolicy::Auto) => RepublishAction::Release,
        (DeploymentReleaseStatus::Draft, RepublishPolicy::Manual)
        | (DeploymentReleaseStatus::Rejected, RepublishPolicy::Manual) => RepublishAction::Review,
        (DeploymentReleaseStatus::PendingReview | DeploymentReleaseStatus::Active, _) => {
            RepublishAction::Conflict
        }
    }
}

fn republish_audit_action(action: RepublishAction, release_pending: bool) -> &'static str {
    match (action, release_pending) {
        (RepublishAction::Review, _) => "deployment.republish_review_requested",
        (RepublishAction::Release, true) => "deployment.republish_queued",
        (RepublishAction::Release, false) => "deployment.republished",
        (RepublishAction::Conflict, _) => unreachable!(),
    }
}

fn map_state_error(error: DeploymentStateError, op: &'static str) -> AppError {
    match error {
        DeploymentStateError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        other => AppError::Conflict {
            op,
            message: other.to_string(),
        },
    }
}

/// POST /api/v1/admin/deployments/{deployment_id}/withdraw
///
/// Immediately removes every route for the deployment, preserves its stored
/// data, and invalidates the previous review by returning it to draft.
pub async fn withdraw(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<GovernanceReason>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.deployments.withdraw";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deployment = deployments::get_by_id_for_update(&transaction, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let reason = optional_reason(body.and_then(|Json(body)| body.reason));
    let deployment = delivery::remove_publication(
        &transaction,
        deployment,
        PublicationRemovalKind::PlatformAdmin,
    )
    .await
    .map_err(|error| {
        crate::features::api::v1::projects::deployments::map_delivery_error(error, OP)
    })?;
    audits::create_platform_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(deployment.team_id),
            action: "deployment.withdrawn".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: reason.clone(),
            metadata: json!({
                "platform_admin": true,
                "project_id": deployment.project_id,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let project = projects::get_by_id_any(&transaction, deployment.project_id)
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
            actor_user_id: session.data.user_id,
            action: "deployment.withdrawn",
            reason: reason.clone(),
            target_url: format!("/projects/{}/deployments/{}", project.id, deployment.id),
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
    let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
    route_invalidation::invalidate_deployment_best_effort(db, &secret_key, deployment.id, OP).await;

    Ok(ok_response(json!({
        "deployment": deployment_view(&deployment),
        "reason": reason,
    })))
}

/// POST /api/v1/admin/deployments/{deployment_id}/republish
pub async fn republish(
    State(state): State<ControlApiState>,
    session: Session,
    Path(deployment_id): Path<Uuid>,
    body: Option<Json<GovernanceReason>>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.deployments.republish";
    let db = super::database(&state, OP)?;
    let reason = optional_reason(body.and_then(|Json(body)| body.reason));
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let mut target = deployments::get_by_id_for_update(&transaction, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    if !matches!(target.build_status, DeploymentBuildStatus::Ready)
        || !matches!(
            target.serve_status,
            DeploymentServeStatus::Ready | DeploymentServeStatus::Retired
        )
    {
        return Err(AppError::Conflict {
            op: OP,
            message: "only deployments with ready build and Serve artifact can be republished"
                .to_owned(),
        });
    }
    if matches!(target.serve_status, DeploymentServeStatus::Retired)
        && !deployments::was_serve_ready(&transaction, target.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        return Err(AppError::Conflict {
            op: OP,
            message: "retired deployment never reached Serve Ready".to_owned(),
        });
    }
    let policy = deployments::review_policy_for_team(&transaction, target.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let mode = match policy.mode_for(&target.environment) {
        ReviewMode::Manual => RepublishPolicy::Manual,
        ReviewMode::Auto => RepublishPolicy::Auto,
    };
    let action = republish_action(&target.release_status, mode);
    if matches!(action, RepublishAction::Conflict) {
        return Err(AppError::Conflict {
            op: OP,
            message: "deployment is already active or waiting for review".to_owned(),
        });
    }

    let mut release_pending = false;
    let mut review_id = None;
    match action {
        RepublishAction::Review => {
            let (updated, review) =
                deployments::request_review(&transaction, target, Some(session.data.user_id))
                    .await
                    .map_err(|error| map_state_error(error, OP))?;
            target = updated;
            review_id = Some(review.id);
        }
        RepublishAction::Release => {
            // A rejected deployment is first returned through the normal
            // review state machine; auto policy then approves it without
            // creating an administrator-authored review decision.
            if matches!(target.release_status, DeploymentReleaseStatus::Rejected) {
                target = deployments::transition_release(
                    &transaction,
                    target,
                    DeploymentReleaseStatus::PendingReview,
                    json!({ "republish": true, "policy": "auto" }),
                )
                .await
                .map_err(|error| map_state_error(error, OP))?;
                target = deployments::transition_release(
                    &transaction,
                    target,
                    DeploymentReleaseStatus::Approved,
                    json!({ "republish": true, "policy": "auto" }),
                )
                .await
                .map_err(|error| map_state_error(error, OP))?;
            }
            let outcome = delivery::request_release(
                &transaction,
                target,
                ReleaseReason::Promote,
                session.data.user_id,
                AuditEventVisibility::Platform,
            )
            .await
            .map_err(|error| {
                crate::features::api::v1::projects::deployments::map_delivery_error(error, OP)
            })?;
            (target, release_pending) = match outcome {
                delivery::ReleaseRequestOutcome::Activated(item) => (item, false),
                delivery::ReleaseRequestOutcome::SyncQueued(item) => (item, true),
            };
        }
        RepublishAction::Conflict => unreachable!(),
    }
    let audit_action = republish_audit_action(action, release_pending);
    audits::create_platform_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(target.team_id),
            action: audit_action.to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(target.id),
            result: AuditEventResult::Success,
            reason: reason.clone(),
            metadata: json!({ "platform_admin": true, "project_id": target.project_id, "release_pending": release_pending, "review_id": review_id }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let project = projects::get_by_id_any(&transaction, target.project_id)
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
            actor_user_id: session.data.user_id,
            action: audit_action,
            reason: reason.clone(),
            target_url: format!("/projects/{}/deployments/{}", project.id, target.id),
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
    Ok(ok_response(json!({
        "deployment": deployment_view(&target),
        "release_status": deployments::release_status_value(&target.release_status),
        "release_pending": release_pending,
        "review_id": review_id,
        "reason": reason,
    })))
}

fn deployment_view(deployment: &deployment::Model) -> serde_json::Value {
    json!({
        "id": deployment.id,
        "project_id": deployment.project_id,
        "release_status": deployments::release_status_value(&deployment.release_status),
        "serve_status": deployments::serve_status_value(&deployment.serve_status),
    })
}

#[cfg(test)]
mod tests {
    use crate::infra::database::entity::DeploymentReleaseStatus;

    use super::{
        RepublishAction, RepublishPolicy, optional_reason, republish_action, republish_audit_action,
    };

    #[test]
    fn administrator_reason_is_optional_and_normalized() {
        assert_eq!(optional_reason(None), None);
        assert_eq!(optional_reason(Some("   ".to_owned())), None);
        assert_eq!(
            optional_reason(Some("  policy violation  ".to_owned())),
            Some("policy violation".to_owned()),
        );
    }

    #[test]
    fn republish_policy_preserves_review_gates_and_conflicts() {
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Approved, RepublishPolicy::Manual),
            RepublishAction::Release
        );
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Draft, RepublishPolicy::Manual),
            RepublishAction::Review
        );
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Draft, RepublishPolicy::Auto),
            RepublishAction::Release
        );
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Rejected, RepublishPolicy::Manual),
            RepublishAction::Review
        );
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Rejected, RepublishPolicy::Auto),
            RepublishAction::Release
        );
        assert_eq!(
            republish_action(
                &DeploymentReleaseStatus::PendingReview,
                RepublishPolicy::Auto
            ),
            RepublishAction::Conflict
        );
        assert_eq!(
            republish_action(&DeploymentReleaseStatus::Active, RepublishPolicy::Auto),
            RepublishAction::Conflict
        );
    }

    #[test]
    fn republish_audit_action_describes_the_actual_outcome() {
        assert_eq!(
            republish_audit_action(RepublishAction::Review, false),
            "deployment.republish_review_requested"
        );
        assert_eq!(
            republish_audit_action(RepublishAction::Release, true),
            "deployment.republish_queued"
        );
        assert_eq!(
            republish_audit_action(RepublishAction::Release, false),
            "deployment.republished"
        );
    }
}
