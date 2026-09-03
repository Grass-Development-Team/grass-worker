use std::collections::HashSet;

use grass_node_protocol::ServeResources;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseTransaction,
    EntityTrait, QueryFilter, QueryOrder,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments, scheduler,
    },
    infra::database::entity::{
        AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentEnvironment,
        DeploymentReleaseStatus, DeploymentServeStatus, ReleaseReason, deployment,
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
    tx: &DatabaseTransaction,
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
    tx: &DatabaseTransaction,
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
    tx: &DatabaseTransaction,
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
    tx: &DatabaseTransaction,
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
        Some(scheduler::place_deployment(tx, serve_resources(&target)?, None).await?)
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
    tx: &DatabaseTransaction,
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
mod tests {
    use std::collections::HashSet;

    use axum::{
        Extension, Json,
        extract::{Path, State},
    };
    use grass_node_protocol::{ReportServeStatusRequest, ReportedServeStatus};
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database,
        DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, Statement, TransactionTrait,
    };
    use sea_orm_migration::MigratorTrait;
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use crate::{
        domain::{
            deployments::{self, BuildTransition, CreateDeploymentParams},
            scheduler::{Placement, PlacementMode},
        },
        infra::database::{
            entity::{
                AuditActorType, AuditEventResult, AuditEventVisibility, DeploymentBuildStatus,
                DeploymentEnvironment, DeploymentReleaseStatus, DeploymentServeStatus,
                NodeConfigSyncStatus, NodeStatus, PlatformRole, ProjectRuntime, ReleaseReason,
                TeamKind, TeamMemberRole, UserStatus, audit_event, deployment, node, project,
                release, team, team_member, user,
            },
            migrate::Migrator,
        },
        infra::{
            config::ControlApiConfig,
            http::{extractors::Session, middlewares::node_auth::AuthenticatedNode},
        },
        state::ControlApiState,
    };

    use super::{
        DeliveryCandidate, PublicationRemovalKind, ReleaseRequestAction, ReleaseRequestOutcome,
        complete_pending_release, desired_delivery_ids, effective_preview_id,
        publication_removal_is_complete, publication_removal_target, reconcile_environment,
        release_audit_action, release_request_action, request_release,
        transition_unsuccessful_build,
    };

    struct PostgresTestDatabase {
        db: DatabaseConnection,
        admin: DatabaseConnection,
        schema: String,
    }

    impl PostgresTestDatabase {
        async fn start() -> Option<Self> {
            let database_url = std::env::var("GRASS_TEST_DATABASE_URL").ok()?;
            let admin = Database::connect(&database_url).await.unwrap();
            let schema = format!("gw_delivery_{}", Uuid::now_v7().simple());
            admin
                .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
                .await
                .unwrap();

            let mut scoped_url = url::Url::parse(&database_url).unwrap();
            scoped_url
                .query_pairs_mut()
                .append_pair("options", &format!("-csearch_path={schema}"));
            let db = Database::connect(scoped_url.as_str()).await.unwrap();
            let _migration_guard = crate::infra::database::migrate::MIGRATION_TEST_LOCK
                .lock()
                .await;
            Migrator::up(&db, None).await.unwrap();
            Some(Self { db, admin, schema })
        }

        async fn cleanup(self) {
            self.db.close().await.unwrap();
            self.admin
                .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .await
                .unwrap();
            self.admin.close().await.unwrap();
        }
    }

    struct DeliveryFixture {
        user: user::Model,
        node: node::Model,
        project: project::Model,
        old_preview: deployment::Model,
        new_preview: deployment::Model,
        rollback_target: deployment::Model,
        current_production: deployment::Model,
    }

    async fn create_deployment_fixture(
        db: &DatabaseConnection,
        project: &project::Model,
        node_id: Uuid,
        environment: DeploymentEnvironment,
        serve_status: DeploymentServeStatus,
        release_status: DeploymentReleaseStatus,
        created_at: OffsetDateTime,
    ) -> deployment::Model {
        let deployment = deployments::create_deployment(
            db,
            CreateDeploymentParams {
                project: project.clone(),
                environment,
                triggered_by_user_id: None,
                branch: Some("main".to_owned()),
                commit_hash: None,
                commit_message: None,
                preview_host: Some(format!("{}.preview.test", Uuid::now_v7().simple())),
                source_credential_version_id: None,
            },
            Placement {
                node_id,
                overcommitted: false,
                mode: PlacementMode::Automatic,
            },
        )
        .await
        .unwrap();
        let mut active: deployment::ActiveModel = deployment.into();
        active.build_status = Set(DeploymentBuildStatus::Ready);
        active.serve_status = Set(serve_status);
        active.release_status = Set(release_status);
        active.created_at = Set(created_at);
        active.updated_at = Set(created_at);
        if matches!(active.serve_status, Set(DeploymentServeStatus::Retired)) {
            active.serve_node_id = Set(None);
        }
        let deployment = active.update(db).await.unwrap();
        if matches!(deployment.serve_status, DeploymentServeStatus::Ready) {
            deployments::append_event(
                db,
                deployment.id,
                crate::infra::database::entity::DeploymentEventKind::Serve,
                "serve status changed to ready",
                serde_json::json!({ "status": "ready" }),
            )
            .await
            .unwrap();
        }
        deployment
    }

    async fn seed_delivery_fixture(db: &DatabaseConnection) -> DeliveryFixture {
        let now = OffsetDateTime::now_utc();
        let user = user::ActiveModel {
            id: Set(Uuid::now_v7()),
            email: Set(format!("{}@example.test", Uuid::now_v7().simple())),
            display_name: Set(Some("Delivery Tester".to_owned())),
            avatar_version: Set(None),
            status: Set(UserStatus::Active),
            platform_role: Set(PlatformRole::Admin),
            email_verified_at: Set(Some(now)),
            last_login_at: Set(None),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
        let team = team::ActiveModel {
            id: Set(Uuid::now_v7()),
            slug: Set(format!("team-{}", Uuid::now_v7().simple())),
            name: Set("Delivery Team".to_owned()),
            avatar_version: Set(None),
            kind: Set(TeamKind::Team),
            group_id: Set(None),
            explicit_quota_plan_id: Set(None),
            owner_user_id: Set(Some(user.id)),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
        team_member::ActiveModel {
            id: Set(Uuid::now_v7()),
            team_id: Set(team.id),
            user_id: Set(user.id),
            role: Set(TeamMemberRole::Owner),
            invited_by_user_id: Set(None),
            joined_at: Set(now),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
        let node = node::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set("serve-1".to_owned()),
            token_hash: Set("test-token-hash".to_owned()),
            status: Set(NodeStatus::Active),
            build_enabled: Set(true),
            serve_enabled: Set(true),
            build_concurrency: Set(1),
            base_url: Set(Some("http://serve-1.test".to_owned())),
            work_root: Set(None),
            capacity_cpu_millicores: Set(10_000),
            capacity_memory_mb: Set(10_000),
            capacity_disk_mb: Set(10_000),
            max_deployments: Set(20),
            metadata: Set(serde_json::json!({})),
            last_heartbeat_at: Set(Some(now)),
            desired_config: Set(None),
            desired_config_revision: Set(0),
            effective_config: Set(None),
            effective_config_revision: Set(0),
            config_sync_status: Set(NodeConfigSyncStatus::Pending),
            config_sync_error: Set(None),
            node_token_configured: Set(false),
            config_updated_at: Set(None),
            config_applied_at: Set(None),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
        let project = project::ActiveModel {
            id: Set(Uuid::now_v7()),
            team_id: Set(team.id),
            created_by_user_id: Set(None),
            slug: Set(format!("project-{}", Uuid::now_v7().simple())),
            name: Set("Delivery Project".to_owned()),
            runtime: Set(ProjectRuntime::Static),
            repository_url: Set(None),
            default_branch: Set(Some("main".to_owned())),
            install_command: Set(None),
            build_command: Set(None),
            output_directory: Set(None),
            source_config: Set(serde_json::json!({})),
            build_config: Set(serde_json::json!({})),
            archived_at: Set(None),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();

        let old_preview = create_deployment_fixture(
            db,
            &project,
            node.id,
            DeploymentEnvironment::Preview,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Draft,
            now,
        )
        .await;
        let new_preview = create_deployment_fixture(
            db,
            &project,
            node.id,
            DeploymentEnvironment::Preview,
            DeploymentServeStatus::Syncing,
            DeploymentReleaseStatus::Draft,
            now + Duration::seconds(1),
        )
        .await;
        let rollback_target = create_deployment_fixture(
            db,
            &project,
            node.id,
            DeploymentEnvironment::Production,
            DeploymentServeStatus::Retired,
            DeploymentReleaseStatus::Approved,
            now + Duration::seconds(2),
        )
        .await;
        let current_production = create_deployment_fixture(
            db,
            &project,
            node.id,
            DeploymentEnvironment::Production,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Active,
            now + Duration::seconds(3),
        )
        .await;

        DeliveryFixture {
            user,
            node,
            project,
            old_preview,
            new_preview,
            rollback_target,
            current_production,
        }
    }

    async fn reload_deployment<C: ConnectionTrait>(db: &C, id: Uuid) -> deployment::Model {
        deployment::Entity::find_by_id(id)
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    async fn insert_audit_actor_fixture(
        db: &DatabaseConnection,
        actor_type: AuditActorType,
        actor_user_id: Option<Uuid>,
        actor_node_id: Option<Uuid>,
    ) -> Result<audit_event::Model, sea_orm::DbErr> {
        audit_event::ActiveModel {
            id: Set(Uuid::now_v7()),
            actor_user_id: Set(actor_user_id),
            actor_node_id: Set(actor_node_id),
            team_id: Set(None),
            actor_type: Set(actor_type),
            visibility: Set(AuditEventVisibility::Platform),
            action: Set("test.actor_constraint".to_owned()),
            target_type: Set("test".to_owned()),
            target_id: Set(None),
            result: Set(AuditEventResult::Success),
            reason: Set(None),
            metadata: Set(serde_json::json!({})),
            request_id: Set(None),
            source_ip: Set(None),
            user_agent: Set(None),
            http_method: Set(None),
            request_path: Set(None),
            status_code: Set(None),
            duration_ms: Set(None),
            changes: Set(serde_json::json!({})),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(db)
        .await
    }

    async fn set_serve_status(db: &DatabaseConnection, id: Uuid, status: DeploymentServeStatus) {
        let deployment = reload_deployment(db, id).await;
        let mut active: deployment::ActiveModel = deployment.into();
        active.serve_status = Set(status);
        active.update(db).await.unwrap();
    }

    fn candidate(
        environment: DeploymentEnvironment,
        build_status: DeploymentBuildStatus,
        serve_status: DeploymentServeStatus,
        release_status: DeploymentReleaseStatus,
        pending_release: bool,
        created_at: i64,
    ) -> DeliveryCandidate {
        DeliveryCandidate {
            id: Uuid::now_v7(),
            environment,
            build_status,
            serve_status,
            release_status,
            pending_release,
            created_at: OffsetDateTime::from_unix_timestamp(created_at).unwrap(),
        }
    }

    #[test]
    fn rolling_preview_keeps_old_ready_until_new_candidate_is_ready() {
        let old = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Draft,
            false,
            1,
        );
        let syncing = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Syncing,
            DeploymentReleaseStatus::Draft,
            false,
            2,
        );

        assert_eq!(
            desired_delivery_ids(&[old.clone(), syncing.clone()]),
            HashSet::from([old.id, syncing.id])
        );
    }

    #[test]
    fn ready_preview_retires_the_previous_preview() {
        let old = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Draft,
            false,
            1,
        );
        let new = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Draft,
            false,
            2,
        );

        assert_eq!(
            desired_delivery_ids(&[old, new.clone()]),
            HashSet::from([new.id])
        );
    }

    #[test]
    fn production_keeps_active_release_during_candidate_sync() {
        let active = candidate(
            DeploymentEnvironment::Production,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Active,
            false,
            1,
        );
        let syncing = candidate(
            DeploymentEnvironment::Production,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Syncing,
            DeploymentReleaseStatus::PendingReview,
            false,
            2,
        );

        assert_eq!(
            desired_delivery_ids(&[active.clone(), syncing.clone()]),
            HashSet::from([active.id, syncing.id])
        );
    }

    #[test]
    fn pending_release_target_is_kept_even_when_retired() {
        let target = candidate(
            DeploymentEnvironment::Production,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Retired,
            DeploymentReleaseStatus::Approved,
            true,
            1,
        );

        assert_eq!(
            desired_delivery_ids(std::slice::from_ref(&target)),
            HashSet::from([target.id])
        );
    }

    #[test]
    fn effective_preview_is_newest_ready_non_retired_deployment() {
        let old = candidate(
            DeploymentEnvironment::Production,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Active,
            false,
            1,
        );
        let new = candidate(
            DeploymentEnvironment::Production,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::PendingReview,
            false,
            2,
        );

        assert_eq!(effective_preview_id(&[old, new.clone()]), Some(new.id));
    }

    #[test]
    fn withdrawn_newest_preview_never_falls_back_to_an_older_deployment() {
        let old = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Approved,
            false,
            1,
        );
        let withdrawn = candidate(
            DeploymentEnvironment::Preview,
            DeploymentBuildStatus::Ready,
            DeploymentServeStatus::Retired,
            DeploymentReleaseStatus::Approved,
            false,
            2,
        );

        assert_eq!(effective_preview_id(&[old, withdrawn]), None);
    }

    #[test]
    fn publication_removal_preserves_or_invalidates_review_by_actor() {
        assert_eq!(
            publication_removal_target(PublicationRemovalKind::TeamUser),
            DeploymentReleaseStatus::Approved,
        );
        assert_eq!(
            publication_removal_target(PublicationRemovalKind::PlatformAdmin),
            DeploymentReleaseStatus::Draft,
        );
    }

    #[test]
    fn completed_publication_removal_is_idempotent_for_route_sync_retries() {
        assert!(publication_removal_is_complete(
            &DeploymentReleaseStatus::Approved,
            &DeploymentServeStatus::Retired,
            PublicationRemovalKind::TeamUser,
        ));
        assert!(publication_removal_is_complete(
            &DeploymentReleaseStatus::Draft,
            &DeploymentServeStatus::Retired,
            PublicationRemovalKind::PlatformAdmin,
        ));
        assert!(!publication_removal_is_complete(
            &DeploymentReleaseStatus::Active,
            &DeploymentServeStatus::Ready,
            PublicationRemovalKind::TeamUser,
        ));
    }

    #[test]
    fn release_request_queues_targets_that_are_not_currently_served() {
        assert_eq!(
            release_request_action(DeploymentServeStatus::Retired, false),
            ReleaseRequestAction::QueueSync
        );
        assert_eq!(
            release_request_action(DeploymentServeStatus::Ready, false),
            ReleaseRequestAction::QueueSync
        );
        assert_eq!(
            release_request_action(DeploymentServeStatus::Ready, true),
            ReleaseRequestAction::Activate
        );
    }

    #[test]
    fn release_audit_actions_distinguish_queued_and_completed_cutovers() {
        assert_eq!(
            release_audit_action(&ReleaseReason::Promote, true),
            "deployment.promotion_queued"
        );
        assert_eq!(
            release_audit_action(&ReleaseReason::Promote, false),
            "deployment.promoted"
        );
        assert_eq!(
            release_audit_action(&ReleaseReason::Rollback, true),
            "deployment.rollback_queued"
        );
        assert_eq!(
            release_audit_action(&ReleaseReason::Rollback, false),
            "deployment.rolled_back"
        );
    }

    #[tokio::test]
    async fn postgres_rollout_and_rollback_preserve_the_serving_version() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;

        let transaction = test_db.db.begin().await.unwrap();
        reconcile_environment(
            &transaction,
            fixture.project.id,
            DeploymentEnvironment::Preview,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert!(
            reload_deployment(&test_db.db, fixture.old_preview.id)
                .await
                .serve_node_id
                .is_some()
        );

        set_serve_status(
            &test_db.db,
            fixture.new_preview.id,
            DeploymentServeStatus::Ready,
        )
        .await;
        let transaction = test_db.db.begin().await.unwrap();
        reconcile_environment(
            &transaction,
            fixture.project.id,
            DeploymentEnvironment::Preview,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        let retired = reload_deployment(&test_db.db, fixture.old_preview.id).await;
        assert_eq!(retired.serve_status, DeploymentServeStatus::Retired);
        assert_eq!(retired.serve_node_id, None);

        let transaction = test_db.db.begin().await.unwrap();
        let outcome = request_release(
            &transaction,
            fixture.rollback_target.clone(),
            ReleaseReason::Rollback,
            fixture.user.id,
            AuditEventVisibility::Team,
        )
        .await
        .unwrap();
        assert!(matches!(outcome, ReleaseRequestOutcome::SyncQueued(_)));
        transaction.commit().await.unwrap();
        assert_eq!(
            reload_deployment(&test_db.db, fixture.current_production.id)
                .await
                .release_status,
            DeploymentReleaseStatus::Active
        );

        set_serve_status(
            &test_db.db,
            fixture.rollback_target.id,
            DeploymentServeStatus::Ready,
        )
        .await;
        let transaction = test_db.db.begin().await.unwrap();
        let target = reload_deployment(&transaction, fixture.rollback_target.id).await;
        let activated = complete_pending_release(&transaction, target)
            .await
            .unwrap()
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(activated.release_status, DeploymentReleaseStatus::Active);
        assert_eq!(activated.pending_release_reason, None);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_unsuccessful_build_retires_its_serve_assignment() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;
        let candidate = reload_deployment(&test_db.db, fixture.new_preview.id).await;
        let mut active: deployment::ActiveModel = candidate.into();
        active.build_status = Set(DeploymentBuildStatus::Building);
        active.serve_status = Set(DeploymentServeStatus::Pending);
        let candidate = active.update(&test_db.db).await.unwrap();

        let transaction = test_db.db.begin().await.unwrap();
        let retired = transition_unsuccessful_build(
            &transaction,
            candidate,
            BuildTransition {
                to: DeploymentBuildStatus::Failed,
                stage: None,
                failure_code: Some("build_failed".to_owned()),
                failure_message: Some("test failure".to_owned()),
                build_node_id: Some(fixture.node.id),
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(retired.build_status, DeploymentBuildStatus::Failed);
        assert_eq!(retired.serve_status, DeploymentServeStatus::Retired);
        assert_eq!(retired.serve_node_id, None);
        assert!(
            reload_deployment(&test_db.db, fixture.old_preview.id)
                .await
                .serve_node_id
                .is_some()
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_serve_ready_rolls_back_when_release_cutover_fails() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;

        let transaction = test_db.db.begin().await.unwrap();
        request_release(
            &transaction,
            fixture.rollback_target.clone(),
            ReleaseReason::Rollback,
            fixture.user.id,
            AuditEventVisibility::Team,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let target = reload_deployment(&test_db.db, fixture.rollback_target.id).await;
        let mut active: deployment::ActiveModel = target.into();
        active.serve_status = Set(DeploymentServeStatus::Syncing);
        active.release_status = Set(DeploymentReleaseStatus::Rejected);
        active.update(&test_db.db).await.unwrap();

        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        let result = crate::features::api::v1::internal::serve::report_status(
            State(state),
            Extension(AuthenticatedNode(fixture.node.clone())),
            Path(fixture.rollback_target.id),
            Json(ReportServeStatusRequest {
                status: ReportedServeStatus::Ready,
                failure_code: None,
                failure_message: None,
            }),
        )
        .await;

        assert!(result.is_err());
        let target = reload_deployment(&test_db.db, fixture.rollback_target.id).await;
        assert_eq!(target.serve_status, DeploymentServeStatus::Syncing);
        assert_eq!(target.pending_release_reason, Some(ReleaseReason::Rollback));

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_review_rolls_back_when_promote_cannot_be_scheduled() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;
        let target = create_deployment_fixture(
            &test_db.db,
            &fixture.project,
            fixture.node.id,
            DeploymentEnvironment::Production,
            DeploymentServeStatus::Ready,
            DeploymentReleaseStatus::Draft,
            OffsetDateTime::now_utc() + Duration::seconds(10),
        )
        .await;
        let (target, _) = deployments::request_review(&test_db.db, target, None)
            .await
            .unwrap();
        let mut active: deployment::ActiveModel = target.into();
        active.serve_status = Set(DeploymentServeStatus::Retired);
        active.serve_node_id = Set(None);
        let target = active.update(&test_db.db).await.unwrap();

        let mut active_node: node::ActiveModel = fixture.node.clone().into();
        active_node.serve_enabled = Set(false);
        active_node.update(&test_db.db).await.unwrap();

        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        let now = OffsetDateTime::now_utc();
        let result = crate::features::api::v1::admin::reviews::approve(
            State(state),
            Session {
                data: grass_session::SessionData {
                    user_id: fixture.user.id,
                    created_at: now,
                    last_accessed_at: now,
                },
                session_id: "test-session".to_owned(),
            },
            Path(target.id),
            Some(Json(
                crate::features::api::v1::admin::reviews::DecisionRequest { reason: None },
            )),
        )
        .await;

        assert!(result.is_err());
        let target = reload_deployment(&test_db.db, target.id).await;
        assert_eq!(
            target.release_status,
            DeploymentReleaseStatus::PendingReview
        );
        assert!(
            deployments::latest_pending_review(&test_db.db, target.id)
                .await
                .unwrap()
                .is_some()
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_release_request_rejects_a_stale_already_active_target() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;
        let mut active: deployment::ActiveModel = fixture.rollback_target.clone().into();
        active.serve_status = Set(DeploymentServeStatus::Ready);
        active.serve_node_id = Set(Some(fixture.node.id));
        let target = active.update(&test_db.db).await.unwrap();
        let stale_target = target.clone();

        let transaction = test_db.db.begin().await.unwrap();
        let first = request_release(
            &transaction,
            target,
            ReleaseReason::Rollback,
            fixture.user.id,
            AuditEventVisibility::Team,
        )
        .await
        .unwrap();
        assert!(matches!(first, ReleaseRequestOutcome::Activated(_)));
        transaction.commit().await.unwrap();

        let transaction = test_db.db.begin().await.unwrap();
        let second = request_release(
            &transaction,
            stale_target,
            ReleaseReason::Rollback,
            fixture.user.id,
            AuditEventVisibility::Team,
        )
        .await;
        assert!(second.is_err());
        transaction.rollback().await.unwrap();

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_queued_release_rolls_back_when_audit_insert_fails() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;
        release::ActiveModel {
            id: Set(Uuid::now_v7()),
            project_id: Set(fixture.project.id),
            deployment_id: Set(fixture.rollback_target.id),
            environment: Set(DeploymentEnvironment::Production),
            reason: Set(ReleaseReason::Rollback),
            actor_user_id: Set(Some(fixture.user.id)),
            previous_deployment_id: Set(None),
            created_at: Set(OffsetDateTime::now_utc()),
        }
        .insert(&test_db.db)
        .await
        .unwrap();
        test_db
            .db
            .execute_unprepared(
                r#"
CREATE FUNCTION reject_queued_release_audit() RETURNS trigger AS $$
BEGIN
    IF NEW.action = 'deployment.rollback_queued' THEN
        RAISE EXCEPTION 'queued release audit rejected by test';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER reject_queued_release_audit
    BEFORE INSERT ON audit_events
    FOR EACH ROW EXECUTE FUNCTION reject_queued_release_audit();
"#,
            )
            .await
            .unwrap();

        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        let now = OffsetDateTime::now_utc();
        let result = crate::features::api::v1::projects::deployments::rollback(
            State(state),
            Session {
                data: grass_session::SessionData {
                    user_id: fixture.user.id,
                    created_at: now,
                    last_accessed_at: now,
                },
                session_id: "test-session".to_owned(),
            },
            Path((fixture.project.id, fixture.rollback_target.id)),
        )
        .await;

        assert!(result.is_err());
        let target = reload_deployment(&test_db.db, fixture.rollback_target.id).await;
        assert_eq!(target.pending_release_reason, None);
        assert_eq!(target.serve_status, DeploymentServeStatus::Retired);
        assert_eq!(target.serve_node_id, None);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_platform_queued_promotion_completion_stays_platform_visible() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;

        let transaction = test_db.db.begin().await.unwrap();
        let queued = request_release(
            &transaction,
            fixture.rollback_target.clone(),
            ReleaseReason::Promote,
            fixture.user.id,
            AuditEventVisibility::Platform,
        )
        .await
        .unwrap();
        assert!(matches!(queued, ReleaseRequestOutcome::SyncQueued(_)));
        transaction.commit().await.unwrap();

        set_serve_status(
            &test_db.db,
            fixture.rollback_target.id,
            DeploymentServeStatus::Ready,
        )
        .await;
        let transaction = test_db.db.begin().await.unwrap();
        let target = reload_deployment(&transaction, fixture.rollback_target.id).await;
        let activated = complete_pending_release(&transaction, target)
            .await
            .unwrap()
            .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(activated.pending_release_audit_visibility, None);
        let event = audit_event::Entity::find()
            .filter(audit_event::Column::TargetId.eq(activated.id))
            .filter(audit_event::Column::Action.eq("deployment.promoted"))
            .one(&test_db.db)
            .await
            .unwrap()
            .expect("queued promotion completion audit");
        assert_eq!(event.visibility, AuditEventVisibility::Platform);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_audit_actor_constraint_rejects_mismatches_and_allows_fk_nulling() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;

        let user_event = insert_audit_actor_fixture(
            &test_db.db,
            AuditActorType::User,
            Some(fixture.user.id),
            None,
        )
        .await
        .unwrap();
        let node_event = insert_audit_actor_fixture(
            &test_db.db,
            AuditActorType::Node,
            None,
            Some(fixture.node.id),
        )
        .await
        .unwrap();
        insert_audit_actor_fixture(&test_db.db, AuditActorType::System, None, None)
            .await
            .unwrap();
        insert_audit_actor_fixture(&test_db.db, AuditActorType::Anonymous, None, None)
            .await
            .unwrap();

        assert!(
            insert_audit_actor_fixture(
                &test_db.db,
                AuditActorType::User,
                None,
                Some(fixture.node.id),
            )
            .await
            .is_err()
        );
        assert!(
            insert_audit_actor_fixture(
                &test_db.db,
                AuditActorType::Node,
                Some(fixture.user.id),
                None,
            )
            .await
            .is_err()
        );
        assert!(
            insert_audit_actor_fixture(
                &test_db.db,
                AuditActorType::System,
                Some(fixture.user.id),
                None,
            )
            .await
            .is_err()
        );

        user::Entity::delete_by_id(fixture.user.id)
            .exec(&test_db.db)
            .await
            .unwrap();
        node::Entity::delete_by_id(fixture.node.id)
            .exec(&test_db.db)
            .await
            .unwrap();

        let user_event = audit_event::Entity::find_by_id(user_event.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user_event.actor_type, AuditActorType::User);
        assert_eq!(user_event.actor_user_id, None);
        let node_event = audit_event::Entity::find_by_id(node_event.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(node_event.actor_type, AuditActorType::Node);
        assert_eq!(node_event.actor_node_id, None);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_review_rejects_a_retired_target_that_was_never_serve_ready() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_delivery_fixture(&test_db.db).await;
        let target = create_deployment_fixture(
            &test_db.db,
            &fixture.project,
            fixture.node.id,
            DeploymentEnvironment::Production,
            DeploymentServeStatus::Pending,
            DeploymentReleaseStatus::Draft,
            OffsetDateTime::now_utc() + Duration::seconds(20),
        )
        .await;
        let (target, _) = deployments::request_review(&test_db.db, target, None)
            .await
            .unwrap();
        let mut active: deployment::ActiveModel = target.into();
        active.serve_status = Set(DeploymentServeStatus::Retired);
        active.serve_node_id = Set(None);
        let target = active.update(&test_db.db).await.unwrap();

        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        let now = OffsetDateTime::now_utc();
        let result = crate::features::api::v1::admin::reviews::approve(
            State(state),
            Session {
                data: grass_session::SessionData {
                    user_id: fixture.user.id,
                    created_at: now,
                    last_accessed_at: now,
                },
                session_id: "test-session".to_owned(),
            },
            Path(target.id),
            Some(Json(
                crate::features::api::v1::admin::reviews::DecisionRequest::default(),
            )),
        )
        .await;

        assert!(result.is_err());
        let target = reload_deployment(&test_db.db, target.id).await;
        assert_eq!(
            target.release_status,
            DeploymentReleaseStatus::PendingReview
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_delivery_rollout_schema_matches_the_domain_model() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };

        let enum_labels = test_db
            .db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder) AS labels
FROM pg_type t
JOIN pg_enum e ON e.enumtypid = t.oid
JOIN pg_namespace n ON n.oid = t.typnamespace
WHERE t.typname = 'deployment_serve_status'
  AND n.nspname = current_schema()
"#,
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "labels")
            .unwrap();
        assert_eq!(enum_labels, "pending,syncing,ready,failed,retired");

        let columns = test_db
            .db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT column_name, udt_name, is_nullable, column_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'deployments'
  AND column_name LIKE 'pending_release_%'
ORDER BY column_name
"#,
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                (
                    row.try_get::<String>("", "column_name").unwrap(),
                    row.try_get::<String>("", "udt_name").unwrap(),
                    row.try_get::<String>("", "is_nullable").unwrap(),
                    row.try_get::<Option<String>>("", "column_default").unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            columns,
            vec![
                (
                    "pending_release_actor_user_id".to_owned(),
                    "uuid".to_owned(),
                    "YES".to_owned(),
                    None,
                ),
                (
                    "pending_release_audit_visibility".to_owned(),
                    "audit_event_visibility".to_owned(),
                    "YES".to_owned(),
                    None,
                ),
                (
                    "pending_release_reason".to_owned(),
                    "release_reason".to_owned(),
                    "YES".to_owned(),
                    None,
                ),
                (
                    "pending_release_requested_at".to_owned(),
                    "timestamptz".to_owned(),
                    "YES".to_owned(),
                    None,
                ),
            ]
        );

        let constraints = test_db
            .db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT conname, pg_get_constraintdef(oid) AS definition
FROM pg_constraint
WHERE conrelid = 'deployments'::regclass
  AND conname IN (
    'fk_deployments_pending_release_actor_user_id',
    'ck_deployments_pending_release_audit_visibility',
    'ck_deployments_pending_release_reason',
    'ck_deployments_pending_release_requested_at'
  )
ORDER BY conname
"#,
            ))
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                (
                    row.try_get::<String>("", "conname").unwrap(),
                    row.try_get::<String>("", "definition").unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(constraints.len(), 4);
        assert!(constraints.iter().any(|(name, definition)| {
            name == "fk_deployments_pending_release_actor_user_id"
                && definition.contains("FOREIGN KEY (pending_release_actor_user_id)")
                && definition.contains("ON DELETE SET NULL")
        }));
        assert!(
            constraints
                .iter()
                .any(|(name, _)| { name == "ck_deployments_pending_release_reason" })
        );
        assert!(
            constraints
                .iter()
                .any(|(name, _)| { name == "ck_deployments_pending_release_requested_at" })
        );
        assert!(constraints.iter().any(|(name, definition)| {
            name == "ck_deployments_pending_release_audit_visibility"
                && definition.contains("pending_release_reason IS NULL")
                && definition.contains("pending_release_audit_visibility IS NULL")
        }));

        let index_definition = test_db
            .db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
SELECT indexdef
FROM pg_indexes
WHERE schemaname = current_schema()
  AND tablename = 'deployments'
  AND indexname = 'ux_deployments_one_pending_release_per_environment'
"#,
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "indexdef")
            .unwrap();
        assert!(index_definition.contains("UNIQUE INDEX"));
        assert!(index_definition.contains("(project_id, environment)"));
        assert!(index_definition.contains("pending_release_reason IS NOT NULL"));
        assert!(index_definition.contains("deleted_at IS NULL"));

        test_db.cleanup().await;
    }
}
