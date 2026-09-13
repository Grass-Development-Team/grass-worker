use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{delivery, deployments::BuildTransition},
    infra::{
        audit::{self as audits, CreateAuditEventParams},
        database::entity::{AuditEventResult, DeploymentBuildStatus, deployment},
        error::AppError,
        http::deployment_errors::map_delivery_error,
        quota::QuotaService,
    },
};

/// Cache key checked by Nodes to learn a build was canceled server-side.
pub(crate) fn cancel_flag_key(deployment_id: Uuid) -> String {
    format!("deployment:{deployment_id}:cancel")
}

pub(crate) fn cancellation_was_requested(
    status: &DeploymentBuildStatus,
    cancel_flagged: bool,
) -> bool {
    matches!(status, DeploymentBuildStatus::Canceled) || cancel_flagged
}

pub(crate) fn cancellation_releases_build_slot(status: &DeploymentBuildStatus) -> bool {
    matches!(
        status,
        DeploymentBuildStatus::Claimed
            | DeploymentBuildStatus::Queued
            | DeploymentBuildStatus::Building
    )
}

// --- Operations -------------------------------------------------------------
/// Cancels a deployment: validates the transition, sets the cooperative
/// cancel flag for the building Node, releases the concurrency slot, and
/// records the audit event. Shared by the REST handler and the websocket
/// cancel path.
pub(crate) async fn cancel_deployment_core(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    deployment: deployment::Model,
    actor_user_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let was_running = cancellation_releases_build_slot(&deployment.build_status);
    let team_id = deployment.team_id;

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let deployment = delivery::transition_unsuccessful_build(
        &transaction,
        deployment,
        BuildTransition {
            to: DeploymentBuildStatus::Canceled,
            stage: None,
            failure_code: None,
            failure_message: Some("canceled by user".to_owned()),
            build_node_id: None,
        },
    )
    .await
    .map_err(|error| map_delivery_error(error, op))?;
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(team_id),
            action: "deployment.canceled".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
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

    if was_running {
        // Cooperative flag for the Node driving this build; it stops the
        // container and releases the concurrency slot when it sees it.
        use grass_cache::Cache;
        let _ = cache
            .set(
                &cancel_flag_key(deployment.id),
                "1",
                std::time::Duration::from_secs(60 * 60 * 24),
            )
            .await;
        QuotaService::new(db, cache)
            .release_build_slot_once(team_id, deployment.id)
            .await;
    }

    Ok(deployment)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_cancel_owns_the_single_slot_release() {
        for status in [
            DeploymentBuildStatus::Claimed,
            DeploymentBuildStatus::Queued,
            DeploymentBuildStatus::Building,
        ] {
            assert!(cancellation_releases_build_slot(&status));
        }

        // After cancel_deployment_core transitions the row, every later Node
        // report takes the early cancellation branch and cannot reach the
        // terminal-stage slot release.
        assert!(cancellation_was_requested(
            &DeploymentBuildStatus::Canceled,
            false
        ));
        assert!(cancellation_was_requested(
            &DeploymentBuildStatus::Building,
            true
        ));
    }

    #[test]
    fn canceling_a_non_running_deployment_does_not_release_a_slot() {
        for status in [
            DeploymentBuildStatus::Pending,
            DeploymentBuildStatus::Ready,
            DeploymentBuildStatus::Failed,
            DeploymentBuildStatus::Canceled,
        ] {
            assert!(!cancellation_releases_build_slot(&status));
        }
    }
}
