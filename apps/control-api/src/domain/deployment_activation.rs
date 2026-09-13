use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        delivery::{self, ReleaseRequestOutcome},
        deployments::{self, ReadyReleaseAction, ReviewMode},
    },
    infra::{
        audit::{self as audits, CreateAuditEventParams},
        database::entity::{
            AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentReleaseStatus,
            ReleaseReason, deployment,
        },
        error::AppError,
        http::deployment_errors::{map_delivery_error, map_state_error},
    },
};

pub(crate) struct ActivationOutcome {
    pub deployment: deployment::Model,
    pub release_pending: bool,
    pub review_id: Option<Uuid>,
}

pub(crate) async fn activate(
    db: &sea_orm::DatabaseConnection,
    deployment: deployment::Model,
    actor_user_id: Uuid,
    reason: ReleaseReason,
    op: &'static str,
) -> Result<ActivationOutcome, AppError> {
    let project_id = deployment.project_id;
    let team_id = deployment.team_id;
    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::Conflict {
            op,
            message: "only deployments with a ready build can be activated".to_owned(),
        });
    }
    if matches!(deployment.release_status, DeploymentReleaseStatus::Active) {
        return Err(AppError::Conflict {
            op,
            message: "deployment is already active".to_owned(),
        });
    }

    // Production activation must pass the review policy; rejected builds
    // can never activate.
    let policy = deployments::review_policy_for_team(db, deployment.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let review_required = matches!(policy.mode_for(&deployment.environment), ReviewMode::Manual);
    let action =
        activation_action(&deployment.release_status, review_required).map_err(|message| {
            AppError::Conflict {
                op,
                message: message.to_owned(),
            }
        })?;

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let (deployment, release_pending, review_id, audit_action) = match action {
        ActivationAction::RequestReview => {
            let deployment = deployments::get_by_id_for_update(&transaction, deployment.id)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op,
                    source: source.into(),
                })?
                .ok_or_else(|| AppError::NotFound {
                    op,
                    message: "deployment not found".to_owned(),
                })?;
            let (deployment, review) =
                deployments::request_review(&transaction, deployment, Some(actor_user_id))
                    .await
                    .map_err(|error| map_state_error(error, op))?;
            (
                deployment,
                false,
                Some(review.id),
                "deployment.review_requested",
            )
        }
        ActivationAction::Release => {
            let outcome = delivery::request_release(
                &transaction,
                deployment,
                reason.clone(),
                actor_user_id,
                AuditEventVisibility::Team,
            )
            .await
            .map_err(|error| map_delivery_error(error, op))?;
            let (deployment, release_pending) = match outcome {
                ReleaseRequestOutcome::Activated(deployment) => (deployment, false),
                ReleaseRequestOutcome::SyncQueued(deployment) => (deployment, true),
            };
            (
                deployment,
                release_pending,
                None,
                delivery::release_audit_action(&reason, release_pending),
            )
        }
    };
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(team_id),
            action: audit_action.to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "project_id": project_id,
                "release_pending": release_pending,
                "review_id": review_id,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;

    Ok(ActivationOutcome {
        deployment,
        release_pending,
        review_id,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivationAction {
    Release,
    RequestReview,
}

fn activation_action(
    status: &DeploymentReleaseStatus,
    review_required: bool,
) -> Result<ActivationAction, &'static str> {
    match status {
        DeploymentReleaseStatus::Rejected => {
            Err("rejected deployments must be re-submitted for review")
        }
        DeploymentReleaseStatus::PendingReview => Err("deployment is waiting for review"),
        DeploymentReleaseStatus::Draft if review_required => Ok(ActivationAction::RequestReview),
        _ => Ok(ActivationAction::Release),
    }
}

pub(crate) async fn complete_serve_ready(
    transaction: &crate::infra::audit::AuditTransaction,
    deployment: deployment::Model,
) -> Result<(), AppError> {
    const OP: &str = "internal.deployments.auto_activate";
    let project_id = deployment.project_id;
    let environment = deployment.environment.clone();
    if deployment.pending_release_reason.is_some() {
        delivery::complete_pending_release(transaction, deployment)
            .await
            .map_err(|error| map_delivery_error(error, OP))?;
    } else {
        let policy = deployments::review_policy_for_team(transaction, deployment.team_id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let action = deployments::serve_ready_release_action(
            policy.mode_for(&deployment.environment),
            &deployment.release_status,
        );
        if matches!(action, ReadyReleaseAction::Activate) {
            let deployment =
                deployments::activate(transaction, deployment, ReleaseReason::Auto, None)
                    .await
                    .map_err(|error| map_state_error(error, OP))?;
            audits::create_audit_event(
                transaction,
                CreateAuditEventParams {
                    actor_user_id: None,
                    actor_node_id: None,
                    team_id: Some(deployment.team_id),
                    action: "deployment.auto_activated".to_owned(),
                    target_type: "deployment".to_owned(),
                    target_id: Some(deployment.id),
                    result: AuditEventResult::Success,
                    reason: None,
                    metadata: json!({
                        "project_id": project_id,
                        "environment": deployments::environment_value(&environment),
                    }),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        }
    };
    delivery::reconcile_environment(transaction, project_id, environment)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_review_drafts_are_submitted_instead_of_rejected() {
        assert_eq!(
            activation_action(&DeploymentReleaseStatus::Draft, true),
            Ok(ActivationAction::RequestReview)
        );
        assert_eq!(
            activation_action(&DeploymentReleaseStatus::Draft, false),
            Ok(ActivationAction::Release)
        );
    }

    #[test]
    fn pending_and_rejected_deployments_cannot_be_activated_directly() {
        assert_eq!(
            activation_action(&DeploymentReleaseStatus::PendingReview, true),
            Err("deployment is waiting for review")
        );
        assert_eq!(
            activation_action(&DeploymentReleaseStatus::Rejected, true),
            Err("rejected deployments must be re-submitted for review")
        );
    }
}
