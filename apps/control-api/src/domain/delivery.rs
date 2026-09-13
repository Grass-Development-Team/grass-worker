use grass_node_protocol::ServeResources;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder,
};
use std::collections::HashSet;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{deployments, scheduler},
    infra::{
        audit::{AuditTransaction, CreateAuditEventParams},
        database::entity::{
            AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentEnvironment,
            DeploymentReleaseStatus, DeploymentServeStatus, ReleaseReason, deployment,
        },
    },
};

#[derive(Debug, thiserror::Error)]
pub enum DeliveryError {
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Schedule(#[from] scheduler::ScheduleError),
    #[error(transparent)]
    State(#[from] deployments::DeploymentStateError),
    #[error("another release operation is already waiting for Serve synchronization")]
    ReleaseAlreadyPending,
    #[error("deployment has invalid Serve resource data")]
    InvalidResources,
    #[error("delivery reconciliation requires a failed or canceled build transition")]
    InvalidUnsuccessfulBuildTransition,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseRequestAction {
    Activate,
    QueueSync,
}

#[derive(Debug)]
pub enum ReleaseRequestOutcome {
    Activated(deployment::Model),
    SyncQueued(deployment::Model),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationRemovalKind {
    TeamUser,
    PlatformAdmin,
}

pub fn publication_removal_target(kind: PublicationRemovalKind) -> DeploymentReleaseStatus {
    match kind {
        PublicationRemovalKind::TeamUser => DeploymentReleaseStatus::Approved,
        PublicationRemovalKind::PlatformAdmin => DeploymentReleaseStatus::Draft,
    }
}

fn publication_removal_is_complete(
    release_status: &DeploymentReleaseStatus,
    serve_status: &DeploymentServeStatus,
    kind: PublicationRemovalKind,
) -> bool {
    release_status == &publication_removal_target(kind)
        && matches!(serve_status, DeploymentServeStatus::Retired)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryCandidate {
    pub id: Uuid,
    pub environment: DeploymentEnvironment,
    pub build_status: DeploymentBuildStatus,
    pub serve_status: DeploymentServeStatus,
    pub release_status: DeploymentReleaseStatus,
    pub pending_release: bool,
    pub created_at: OffsetDateTime,
}

/// Returns the deployments that must keep a Serve assignment for one project
/// environment. The newest ready-build candidate is delivered first, while
/// its previous ready deployment remains assigned until the new one is ready.
pub fn desired_delivery_ids(candidates: &[DeliveryCandidate]) -> HashSet<Uuid> {
    let mut desired = HashSet::new();

    for candidate in candidates {
        if matches!(
            candidate.build_status,
            DeploymentBuildStatus::Pending
                | DeploymentBuildStatus::Claimed
                | DeploymentBuildStatus::Queued
                | DeploymentBuildStatus::Building
        ) {
            desired.insert(candidate.id);
        }
        if candidate.pending_release
            || (matches!(candidate.environment, DeploymentEnvironment::Production)
                && matches!(candidate.release_status, DeploymentReleaseStatus::Active))
        {
            desired.insert(candidate.id);
        }
    }

    let mut ready_builds = candidates
        .iter()
        .filter(|candidate| matches!(candidate.build_status, DeploymentBuildStatus::Ready))
        .filter(|candidate| {
            !matches!(candidate.serve_status, DeploymentServeStatus::Retired)
                || candidate.pending_release
        })
        .collect::<Vec<_>>();
    ready_builds.sort_by_key(|candidate| (candidate.created_at, candidate.id));

    let Some(newest) = ready_builds.pop() else {
        return desired;
    };
    desired.insert(newest.id);

    if !matches!(newest.serve_status, DeploymentServeStatus::Ready)
        && let Some(previous) = ready_builds
            .into_iter()
            .rev()
            .find(|candidate| matches!(candidate.serve_status, DeploymentServeStatus::Ready))
    {
        desired.insert(previous.id);
    }

    desired
}

pub fn effective_preview_id(candidates: &[DeliveryCandidate]) -> Option<Uuid> {
    candidates
        .iter()
        .filter(|candidate| matches!(candidate.build_status, DeploymentBuildStatus::Ready))
        .max_by_key(|candidate| (candidate.created_at, candidate.id))
        .filter(|candidate| matches!(candidate.serve_status, DeploymentServeStatus::Ready))
        .map(|candidate| candidate.id)
}

pub fn release_request_action(
    serve_status: DeploymentServeStatus,
    has_assignment: bool,
) -> ReleaseRequestAction {
    if has_assignment && matches!(serve_status, DeploymentServeStatus::Ready) {
        ReleaseRequestAction::Activate
    } else {
        ReleaseRequestAction::QueueSync
    }
}

pub fn release_audit_action(reason: &ReleaseReason, queued: bool) -> &'static str {
    match (reason, queued) {
        (ReleaseReason::Promote, true) => "deployment.promotion_queued",
        (ReleaseReason::Promote, false) => "deployment.promoted",
        (ReleaseReason::Rollback, true) => "deployment.rollback_queued",
        (ReleaseReason::Rollback, false) => "deployment.rolled_back",
        (ReleaseReason::Auto, _) => "deployment.auto_activated",
    }
}

fn serve_resources(item: &deployment::Model) -> Result<ServeResources, DeliveryError> {
    Ok(ServeResources {
        cpu_millicores: item
            .serve_cpu_millicores
            .try_into()
            .map_err(|_| DeliveryError::InvalidResources)?,
        memory_mb: item
            .serve_memory_mb
            .try_into()
            .map_err(|_| DeliveryError::InvalidResources)?,
        disk_mb: item
            .serve_disk_mb
            .try_into()
            .map_err(|_| DeliveryError::InvalidResources)?,
    })
}

pub fn candidate_from_model(item: &deployment::Model) -> DeliveryCandidate {
    DeliveryCandidate {
        id: item.id,
        environment: item.environment.clone(),
        build_status: item.build_status.clone(),
        serve_status: item.serve_status.clone(),
        release_status: item.release_status.clone(),
        pending_release: item.pending_release_reason.is_some(),
        created_at: item.created_at,
    }
}

pub async fn effective_preview<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    environment: DeploymentEnvironment,
) -> Result<Option<deployment::Model>, sea_orm::DbErr> {
    deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::Environment.eq(environment))
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
        .filter(deployment::Column::PreviewHost.is_not_null())
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_desc(deployment::Column::CreatedAt)
        .order_by_desc(deployment::Column::Id)
        .one(db)
        .await
        .map(|deployment| {
            deployment.filter(|deployment| {
                matches!(deployment.serve_status, DeploymentServeStatus::Ready)
            })
        })
}

/// Removes all routes for the current publication while retaining its build,
/// artifact, log, and review records. This operation never activates an older
/// deployment.
pub async fn remove_publication(
    tx: &AuditTransaction,
    target: deployment::Model,
    kind: PublicationRemovalKind,
) -> Result<deployment::Model, DeliveryError> {
    scheduler::lock_placement(tx).await?;
    let target = deployments::get_by_id_for_update(tx, target.id)
        .await?
        .ok_or_else(|| DeliveryError::Other(anyhow::anyhow!("deployment disappeared")))?;
    if publication_removal_is_complete(&target.release_status, &target.serve_status, kind) {
        return Ok(target);
    }
    let project_id = target.project_id;
    let environment = target.environment.clone();
    let target = deployments::transition_release(
        tx,
        target,
        publication_removal_target(kind),
        serde_json::json!({
            "publication_removed_by": match kind {
                PublicationRemovalKind::TeamUser => "team_user",
                PublicationRemovalKind::PlatformAdmin => "platform_admin",
            },
        }),
    )
    .await?;
    let target = if matches!(target.serve_status, DeploymentServeStatus::Retired) {
        target
    } else {
        deployments::transition_serve(
            tx,
            target,
            deployments::ServeTransition {
                to: DeploymentServeStatus::Retired,
                failure_code: None,
                failure_message: None,
            },
        )
        .await?
    };
    let mut active: deployment::ActiveModel = target.into();
    active.serve_node_id = Set(None);
    active.pending_release_reason = Set(None);
    active.pending_release_actor_user_id = Set(None);
    active.pending_release_audit_visibility = Set(None);
    active.pending_release_requested_at = Set(None);
    let removed = active.update(tx).await?;
    reconcile_environment(tx, project_id, environment).await?;
    Ok(removed)
}

/// Reconciles the Serve assignments for one project environment while holding
/// the same placement lock used by the scheduler.
pub async fn reconcile_environment(
    tx: &AuditTransaction,
    project_id: Uuid,
    environment: DeploymentEnvironment,
) -> Result<(), DeliveryError> {
    scheduler::lock_placement(tx).await?;
    let items = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::Environment.eq(environment))
        .filter(deployment::Column::DeletedAt.is_null())
        .all(tx)
        .await?;
    let desired = desired_delivery_ids(&items.iter().map(candidate_from_model).collect::<Vec<_>>());

    for item in items {
        let in_progress = matches!(
            item.build_status,
            DeploymentBuildStatus::Pending
                | DeploymentBuildStatus::Claimed
                | DeploymentBuildStatus::Queued
                | DeploymentBuildStatus::Building
        );
        if in_progress || desired.contains(&item.id) {
            continue;
        }
        if item.serve_node_id.is_none()
            && matches!(item.serve_status, DeploymentServeStatus::Retired)
        {
            continue;
        }

        let retired = if matches!(item.serve_status, DeploymentServeStatus::Retired) {
            item
        } else {
            deployments::transition_serve(
                tx,
                item,
                deployments::ServeTransition {
                    to: DeploymentServeStatus::Retired,
                    failure_code: None,
                    failure_message: None,
                },
            )
            .await?
        };
        let mut active: deployment::ActiveModel = retired.into();
        active.serve_node_id = Set(None);
        active.update(tx).await?;
    }

    Ok(())
}

/// Applies an unsuccessful terminal build transition and releases its Serve
/// assignment in the same transaction.
pub async fn transition_unsuccessful_build(
    tx: &AuditTransaction,
    target: deployment::Model,
    transition: deployments::BuildTransition,
) -> Result<deployment::Model, DeliveryError> {
    if !matches!(
        transition.to,
        DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
    ) {
        return Err(DeliveryError::InvalidUnsuccessfulBuildTransition);
    }

    scheduler::lock_placement(tx).await?;
    let deployment_id = target.id;
    let target = deployments::get_by_id_for_update(tx, deployment_id)
        .await?
        .ok_or_else(|| DeliveryError::Other(anyhow::anyhow!("deployment disappeared")))?;
    let project_id = target.project_id;
    let environment = target.environment.clone();
    deployments::transition_build(tx, target, transition).await?;
    reconcile_environment(tx, project_id, environment).await?;
    deployment::Entity::find_by_id(deployment_id)
        .one(tx)
        .await?
        .ok_or_else(|| DeliveryError::Other(anyhow::anyhow!("deployment disappeared")))
}

pub async fn request_release(
    tx: &AuditTransaction,
    target: deployment::Model,
    reason: ReleaseReason,
    actor_user_id: Uuid,
    audit_visibility: AuditEventVisibility,
) -> Result<ReleaseRequestOutcome, DeliveryError> {
    scheduler::lock_placement(tx).await?;
    let target = deployments::get_by_id_for_update(tx, target.id)
        .await?
        .ok_or_else(|| DeliveryError::Other(anyhow::anyhow!("deployment disappeared")))?;
    let pending = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(target.project_id))
        .filter(deployment::Column::Environment.eq(target.environment.clone()))
        .filter(deployment::Column::PendingReleaseReason.is_not_null())
        .filter(deployment::Column::DeletedAt.is_null())
        .one(tx)
        .await?;
    if pending.is_some() {
        return Err(DeliveryError::ReleaseAlreadyPending);
    }

    if matches!(
        release_request_action(target.serve_status.clone(), target.serve_node_id.is_some()),
        ReleaseRequestAction::Activate
    ) {
        let activated = deployments::activate(tx, target, reason, Some(actor_user_id)).await?;
        reconcile_environment(tx, activated.project_id, activated.environment.clone()).await?;
        return Ok(ReleaseRequestOutcome::Activated(activated));
    }

    let placement = if target.serve_node_id.is_none()
        || matches!(target.serve_status, DeploymentServeStatus::Retired)
    {
        Some(
            scheduler::place_deployment_in_region(
                tx,
                serve_resources(&target)?,
                None,
                Some(&target.region),
            )
            .await?,
        )
    } else {
        None
    };
    let project_id = target.project_id;
    let environment = target.environment.clone();
    let deployment_id = target.id;
    let now = OffsetDateTime::now_utc();
    let mut active: deployment::ActiveModel = target.into();
    if let Some(placement) = placement {
        active.serve_node_id = Set(Some(placement.node_id));
        active.overcommitted = Set(placement.overcommitted);
        active.serve_status = Set(DeploymentServeStatus::Pending);
        active.serve_started_at = Set(None);
        active.serve_finished_at = Set(None);
        active.serve_failure_code = Set(None);
        active.serve_failure_message = Set(None);
    }
    active.pending_release_reason = Set(Some(reason.clone()));
    active.pending_release_actor_user_id = Set(Some(actor_user_id));
    active.pending_release_audit_visibility = Set(Some(audit_visibility));
    active.pending_release_requested_at = Set(Some(now));
    let queued = active.update(tx).await?;
    deployments::append_event(
        tx,
        deployment_id,
        crate::infra::database::entity::DeploymentEventKind::Release,
        "release waiting for Serve synchronization",
        serde_json::json!({
            "reason": deployments::release_reason_value(&reason),
            "serve_node_id": queued.serve_node_id,
        }),
    )
    .await?;
    reconcile_environment(tx, project_id, environment).await?;
    Ok(ReleaseRequestOutcome::SyncQueued(queued))
}

pub async fn complete_pending_release(
    tx: &AuditTransaction,
    target: deployment::Model,
) -> Result<Option<deployment::Model>, DeliveryError> {
    let Some(reason) = target.pending_release_reason.clone() else {
        return Ok(None);
    };
    if !matches!(target.serve_status, DeploymentServeStatus::Ready) {
        return Ok(None);
    }

    let actor_user_id = target.pending_release_actor_user_id;
    let audit_visibility = target
        .pending_release_audit_visibility
        .clone()
        .unwrap_or(AuditEventVisibility::Platform);
    let activated = deployments::activate(tx, target, reason.clone(), actor_user_id).await?;
    let project_id = activated.project_id;
    let environment = activated.environment.clone();
    let mut active: deployment::ActiveModel = activated.into();
    active.pending_release_reason = Set(None);
    active.pending_release_actor_user_id = Set(None);
    active.pending_release_audit_visibility = Set(None);
    active.pending_release_requested_at = Set(None);
    let activated = active.update(tx).await?;
    reconcile_environment(tx, project_id, environment).await?;
    audits::create_audit_event_with_visibility(
        tx,
        CreateAuditEventParams {
            actor_user_id,
            actor_node_id: None,
            team_id: Some(activated.team_id),
            action: release_audit_action(&reason, false).to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(activated.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: serde_json::json!({
                "project_id": activated.project_id,
                "release_pending": false,
                "completed_after_sync": true,
            }),
        },
        audit_visibility,
    )
    .await?;
    Ok(Some(activated))
}

#[cfg(test)]
mod tests;
