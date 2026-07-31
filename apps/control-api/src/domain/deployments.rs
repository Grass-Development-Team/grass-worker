//! Database-backed deployment business functions.
//!
//! Deployments carry two independent lifecycles: the build status driven by
//! Nodes, and the release status driven by people and review policy. Every
//! transition is validated against the state machine, appended to
//! `deployment_events`, and activation switches are transactional so one
//! project environment never has two active deployments.

use grass_node_protocol::ServeResources;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::domain::scheduler::Placement;
use crate::infra::database::entity::{
    DeploymentBuildStatus, DeploymentEnvironment, DeploymentEventKind, DeploymentReleaseStatus,
    DeploymentReviewStatus, DeploymentServeStatus, ProjectRuntime, ReleaseReason, deployment,
    deployment_artifact, deployment_event, deployment_review, project, release, team, team_group,
};

// --- State machine ----------------------------------------------------------

/// Valid build status transitions:
///
/// ```text
/// pending → claimed → queued → building → ready
///        ↘ canceled          ↘ failed / canceled
/// ```
pub fn can_transition_build(from: &DeploymentBuildStatus, to: &DeploymentBuildStatus) -> bool {
    use DeploymentBuildStatus as B;
    matches!(
        (from, to),
        (B::Pending, B::Claimed)
            | (B::Pending, B::Canceled)
            | (B::Pending, B::Failed)
            | (B::Claimed, B::Queued)
            | (B::Claimed, B::Building)
            | (B::Claimed, B::Failed)
            | (B::Claimed, B::Canceled)
            | (B::Queued, B::Building)
            | (B::Queued, B::Failed)
            | (B::Queued, B::Canceled)
            | (B::Building, B::Ready)
            | (B::Building, B::Failed)
            | (B::Building, B::Canceled)
    )
}

/// Valid release status transitions. `active → approved` happens when a
/// newer deployment takes over the environment.
pub fn can_transition_release(
    from: &DeploymentReleaseStatus,
    to: &DeploymentReleaseStatus,
) -> bool {
    use DeploymentReleaseStatus as R;
    matches!(
        (from, to),
        (R::Draft, R::PendingReview)
            | (R::Draft, R::Active)
            | (R::PendingReview, R::Approved)
            | (R::PendingReview, R::Rejected)
            | (R::Rejected, R::PendingReview)
            | (R::Approved, R::Active)
            | (R::Active, R::Approved)
            | (R::Active, R::Draft)
    )
}

#[derive(Debug, thiserror::Error)]
pub enum DeploymentStateError {
    #[error("invalid build status transition from {from} to {to}")]
    InvalidBuildTransition { from: String, to: String },
    #[error("invalid release status transition from {from} to {to}")]
    InvalidReleaseTransition { from: String, to: String },
    #[error("invalid serve status transition from {from} to {to}")]
    #[allow(dead_code)] // Constructed by the P3.2 Serve status endpoint.
    InvalidServeTransition { from: String, to: String },
    #[error("only ready deployments can enter the release flow")]
    BuildNotReady,
    #[error("only deployments with a ready Serve artifact can enter the release flow")]
    ServeNotReady,
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

pub fn build_status_value(status: &DeploymentBuildStatus) -> &'static str {
    match status {
        DeploymentBuildStatus::Pending => "pending",
        DeploymentBuildStatus::Claimed => "claimed",
        DeploymentBuildStatus::Queued => "queued",
        DeploymentBuildStatus::Building => "building",
        DeploymentBuildStatus::Ready => "ready",
        DeploymentBuildStatus::Failed => "failed",
        DeploymentBuildStatus::Canceled => "canceled",
    }
}

pub fn release_status_value(status: &DeploymentReleaseStatus) -> &'static str {
    match status {
        DeploymentReleaseStatus::Draft => "draft",
        DeploymentReleaseStatus::PendingReview => "pending_review",
        DeploymentReleaseStatus::Approved => "approved",
        DeploymentReleaseStatus::Rejected => "rejected",
        DeploymentReleaseStatus::Active => "active",
    }
}

pub fn environment_value(environment: &DeploymentEnvironment) -> &'static str {
    match environment {
        DeploymentEnvironment::Production => "production",
        DeploymentEnvironment::Preview => "preview",
    }
}

pub fn can_transition_serve(from: &DeploymentServeStatus, to: &DeploymentServeStatus) -> bool {
    use DeploymentServeStatus as S;
    matches!(
        (from, to),
        (S::Pending | S::Failed | S::Ready, S::Syncing)
            | (S::Syncing, S::Ready | S::Failed)
            | (S::Pending | S::Syncing | S::Failed | S::Ready, S::Retired)
            | (S::Retired, S::Pending)
    )
}

pub fn serve_status_value(status: &DeploymentServeStatus) -> &'static str {
    match status {
        DeploymentServeStatus::Pending => "pending",
        DeploymentServeStatus::Syncing => "syncing",
        DeploymentServeStatus::Ready => "ready",
        DeploymentServeStatus::Failed => "failed",
        DeploymentServeStatus::Retired => "retired",
    }
}

fn validate_release_readiness(
    from: &DeploymentReleaseStatus,
    to: &DeploymentReleaseStatus,
    build_status: &DeploymentBuildStatus,
    serve_status: &DeploymentServeStatus,
) -> Result<(), DeploymentStateError> {
    if matches!(from, DeploymentReleaseStatus::Active)
        && matches!(
            to,
            DeploymentReleaseStatus::Approved | DeploymentReleaseStatus::Draft
        )
    {
        return Ok(());
    }
    if !matches!(build_status, DeploymentBuildStatus::Ready) {
        return Err(DeploymentStateError::BuildNotReady);
    }
    if matches!(
        (from, to),
        (
            DeploymentReleaseStatus::Draft,
            DeploymentReleaseStatus::PendingReview
        )
    ) {
        return Ok(());
    }
    if matches!(serve_status, DeploymentServeStatus::Retired)
        && matches!(
            (from, to),
            (
                DeploymentReleaseStatus::PendingReview,
                DeploymentReleaseStatus::Approved | DeploymentReleaseStatus::Rejected
            )
        )
    {
        return Ok(());
    }
    if !matches!(serve_status, DeploymentServeStatus::Ready) {
        return Err(DeploymentStateError::ServeNotReady);
    }
    Ok(())
}

pub fn runtime_serve_resources(runtime: &ProjectRuntime) -> ServeResources {
    match runtime {
        ProjectRuntime::Ssr => ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        },
        _ => ServeResources {
            cpu_millicores: 50,
            memory_mb: 64,
            disk_mb: 256,
        },
    }
}

/// Artifacts uploaded before Serve scheduling did not record unpacked bytes.
/// Zero is reserved as the wire-level sentinel for that legacy metadata.
pub fn artifact_unpacked_size_bytes(manifest: &serde_json::Value) -> Option<u64> {
    match manifest.get("unpacked_size_bytes") {
        Some(value) => value.as_u64(),
        None => Some(0),
    }
}

// --- Creation ---------------------------------------------------------------

pub struct CreateDeploymentParams {
    pub project: project::Model,
    pub environment: DeploymentEnvironment,
    pub triggered_by_user_id: Option<Uuid>,
    pub branch: Option<String>,
    pub commit_hash: Option<String>,
    pub commit_message: Option<String>,
    pub preview_host: Option<String>,
    pub source_credential_version_id: Option<Uuid>,
}

/// Creates a deployment snapshotting the project's source and build
/// configuration so later project edits do not change queued builds.
pub async fn create_deployment<C: ConnectionTrait>(
    db: &C,
    params: CreateDeploymentParams,
    placement: Placement,
) -> anyhow::Result<deployment::Model> {
    let now = OffsetDateTime::now_utc();
    let project = params.project;
    let serve_resources = runtime_serve_resources(&project.runtime);

    let deployment = deployment::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(project.id),
        team_id: Set(project.team_id),
        build_node_id: Set(None),
        serve_node_id: Set(Some(placement.node_id)),
        environment: Set(params.environment),
        runtime_kind: Set(project.runtime.clone()),
        build_status: Set(DeploymentBuildStatus::Pending),
        serve_status: Set(DeploymentServeStatus::Pending),
        release_status: Set(DeploymentReleaseStatus::Draft),
        serve_cpu_millicores: Set(i64::try_from(serve_resources.cpu_millicores)?),
        serve_memory_mb: Set(i64::try_from(serve_resources.memory_mb)?),
        serve_disk_mb: Set(i64::try_from(serve_resources.disk_mb)?),
        overcommitted: Set(placement.overcommitted),
        source_repository_url: Set(project.repository_url.clone()),
        source_credential_version_id: Set(params.source_credential_version_id),
        source_branch: Set(params
            .branch
            .or_else(|| project.default_branch.clone())
            .or_else(|| Some("main".to_owned()))),
        commit_hash: Set(params.commit_hash),
        commit_message: Set(params.commit_message),
        triggered_by_user_id: Set(params.triggered_by_user_id),
        install_command: Set(project.install_command.clone()),
        build_command: Set(project.build_command.clone()),
        output_directory: Set(project.output_directory.clone()),
        source_metadata: Set(project.source_config.clone()),
        preview_host: Set(params.preview_host),
        build_stage: Set(None),
        failure_code: Set(None),
        failure_message: Set(None),
        serve_failure_code: Set(None),
        serve_failure_message: Set(None),
        pending_release_reason: Set(None),
        pending_release_actor_user_id: Set(None),
        pending_release_audit_visibility: Set(None),
        pending_release_requested_at: Set(None),
        claimed_at: Set(None),
        build_started_at: Set(None),
        build_finished_at: Set(None),
        serve_started_at: Set(None),
        serve_finished_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    append_event(
        db,
        deployment.id,
        DeploymentEventKind::System,
        "deployment created",
        serde_json::json!({
            "environment": environment_value(&deployment.environment),
        }),
    )
    .await?;

    Ok(deployment)
}

// --- Events -----------------------------------------------------------------

pub async fn append_event<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
    kind: DeploymentEventKind,
    message: &str,
    metadata: serde_json::Value,
) -> anyhow::Result<deployment_event::Model> {
    deployment_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment_id),
        kind: Set(kind),
        message: Set(message.to_owned()),
        metadata: Set(metadata),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

pub async fn list_events<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<Vec<deployment_event::Model>> {
    deployment_event::Entity::find()
        .filter(deployment_event::Column::DeploymentId.eq(deployment_id))
        .order_by_asc(deployment_event::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn was_serve_ready<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<bool> {
    let events = deployment_event::Entity::find()
        .filter(deployment_event::Column::DeploymentId.eq(deployment_id))
        .filter(deployment_event::Column::Kind.eq(DeploymentEventKind::Serve))
        .all(db)
        .await?;
    Ok(events.iter().any(|event| {
        event
            .metadata
            .get("status")
            .and_then(|value| value.as_str())
            == Some("ready")
    }))
}

// --- Queries ----------------------------------------------------------------

pub async fn get_by_id<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<Option<deployment::Model>> {
    deployment::Entity::find()
        .filter(deployment::Column::Id.eq(deployment_id))
        .filter(deployment::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

/// Loads and locks a deployment for release/build finalization. Callers must
/// pass a transaction so the lock is held through the complete state change.
pub async fn get_by_id_for_update<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> Result<Option<deployment::Model>, sea_orm::DbErr> {
    deployment::Entity::find_by_id(deployment_id)
        .filter(deployment::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(db)
        .await
}

pub struct DeploymentListFilter {
    pub environment: Option<DeploymentEnvironment>,
    pub build_status: Option<DeploymentBuildStatus>,
    pub limit: u64,
    pub offset: u64,
}

pub async fn list_for_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    filter: DeploymentListFilter,
) -> anyhow::Result<Vec<deployment::Model>> {
    let mut query = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::DeletedAt.is_null());
    if let Some(environment) = filter.environment {
        query = query.filter(deployment::Column::Environment.eq(environment));
    }
    if let Some(status) = filter.build_status {
        query = query.filter(deployment::Column::BuildStatus.eq(status));
    }
    query
        .order_by_desc(deployment::Column::CreatedAt)
        .limit(filter.limit.clamp(1, 100))
        .offset(filter.offset)
        .all(db)
        .await
        .map_err(Into::into)
}

#[allow(dead_code)] // Wired by the serve resolve API in Milestone 6.
pub async fn find_active<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    environment: DeploymentEnvironment,
) -> anyhow::Result<Option<deployment::Model>> {
    deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::Environment.eq(environment))
        .filter(deployment::Column::ReleaseStatus.eq(DeploymentReleaseStatus::Active))
        .filter(deployment::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

#[allow(dead_code)] // Wired by the serve resolve API in Milestone 6.
pub async fn find_by_preview_host<C: ConnectionTrait>(
    db: &C,
    host: &str,
) -> anyhow::Result<Option<deployment::Model>> {
    deployment::Entity::find()
        .filter(deployment::Column::PreviewHost.eq(host))
        .filter(deployment::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn list_artifacts<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<Vec<deployment_artifact::Model>> {
    deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
        .order_by_asc(deployment_artifact::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

// --- Build status transitions ----------------------------------------------

pub struct BuildTransition {
    pub to: DeploymentBuildStatus,
    pub stage: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub build_node_id: Option<Uuid>,
}

#[allow(dead_code)] // Constructed by the P3.2 Serve status endpoint.
pub struct ServeTransition {
    pub to: DeploymentServeStatus,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
}

#[allow(dead_code)] // Called by the P3.2 Serve status endpoint.
pub async fn transition_serve<C: ConnectionTrait>(
    db: &C,
    deployment: deployment::Model,
    transition: ServeTransition,
) -> Result<deployment::Model, DeploymentStateError> {
    if !can_transition_serve(&deployment.serve_status, &transition.to) {
        return Err(DeploymentStateError::InvalidServeTransition {
            from: serve_status_value(&deployment.serve_status).to_owned(),
            to: serve_status_value(&transition.to).to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    let deployment_id = deployment.id;
    let to_value = serve_status_value(&transition.to);
    let mut active: deployment::ActiveModel = deployment.into();
    active.serve_status = Set(transition.to.clone());
    match transition.to {
        DeploymentServeStatus::Syncing => {
            active.serve_started_at = Set(Some(now));
            active.serve_finished_at = Set(None);
            active.serve_failure_code = Set(None);
            active.serve_failure_message = Set(None);
        }
        DeploymentServeStatus::Ready => {
            active.serve_finished_at = Set(Some(now));
            active.serve_failure_code = Set(None);
            active.serve_failure_message = Set(None);
        }
        DeploymentServeStatus::Failed => {
            active.serve_finished_at = Set(Some(now));
            active.serve_failure_code = Set(transition.failure_code.clone());
            active.serve_failure_message = Set(transition.failure_message.clone());
        }
        DeploymentServeStatus::Pending | DeploymentServeStatus::Retired => {}
    }

    let updated = active.update(db).await?;
    append_event(
        db,
        deployment_id,
        DeploymentEventKind::Serve,
        &format!("serve status changed to {to_value}"),
        serde_json::json!({
            "status": to_value,
            "failure_code": transition.failure_code,
            "failure_message": transition.failure_message,
        }),
    )
    .await
    .map_err(|error| DeploymentStateError::Database(sea_orm::DbErr::Custom(error.to_string())))?;

    Ok(updated)
}

/// Applies a validated build status transition, maintaining lifecycle
/// timestamps and appending a build event.
pub async fn transition_build<C: ConnectionTrait>(
    db: &C,
    deployment: deployment::Model,
    transition: BuildTransition,
) -> Result<deployment::Model, DeploymentStateError> {
    if !can_transition_build(&deployment.build_status, &transition.to) {
        return Err(DeploymentStateError::InvalidBuildTransition {
            from: build_status_value(&deployment.build_status).to_owned(),
            to: build_status_value(&transition.to).to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc();
    let deployment_id = deployment.id;
    let to_value = build_status_value(&transition.to);
    let mut active: deployment::ActiveModel = deployment.into();
    active.build_status = Set(transition.to.clone());
    if let Some(stage) = &transition.stage {
        active.build_stage = Set(Some(stage.clone()));
    }
    match transition.to {
        DeploymentBuildStatus::Claimed => {
            active.claimed_at = Set(Some(now));
            if let Some(node_id) = transition.build_node_id {
                active.build_node_id = Set(Some(node_id));
            }
        }
        DeploymentBuildStatus::Building => {
            active.build_started_at = Set(Some(now));
        }
        DeploymentBuildStatus::Ready
        | DeploymentBuildStatus::Failed
        | DeploymentBuildStatus::Canceled => {
            active.build_finished_at = Set(Some(now));
            active.failure_code = Set(transition.failure_code.clone());
            active.failure_message = Set(transition.failure_message.clone());
        }
        _ => {}
    }

    let updated = active.update(db).await?;
    append_event(
        db,
        deployment_id,
        DeploymentEventKind::Build,
        &format!("build status changed to {to_value}"),
        serde_json::json!({
            "status": to_value,
            "stage": transition.stage,
            "failure_code": transition.failure_code,
            "failure_message": transition.failure_message,
        }),
    )
    .await
    .map_err(|error| DeploymentStateError::Database(sea_orm::DbErr::Custom(error.to_string())))?;

    Ok(updated)
}

/// Updates only the build stage without changing status; used for progress
/// reporting between status transitions.
#[allow(dead_code)] // Wired by the Node stage API in Milestone 6.
pub async fn update_stage<C: ConnectionTrait>(
    db: &C,
    deployment: deployment::Model,
    stage: &str,
) -> anyhow::Result<deployment::Model> {
    let deployment_id = deployment.id;
    let mut active: deployment::ActiveModel = deployment.into();
    active.build_stage = Set(Some(stage.to_owned()));
    let updated = active.update(db).await?;
    append_event(
        db,
        deployment_id,
        DeploymentEventKind::Build,
        &format!("stage changed to {stage}"),
        serde_json::json!({ "stage": stage }),
    )
    .await?;
    Ok(updated)
}

// --- Release transitions ----------------------------------------------------

pub async fn transition_release<C: ConnectionTrait>(
    db: &C,
    deployment: deployment::Model,
    to: DeploymentReleaseStatus,
    metadata: serde_json::Value,
) -> Result<deployment::Model, DeploymentStateError> {
    if !can_transition_release(&deployment.release_status, &to) {
        return Err(DeploymentStateError::InvalidReleaseTransition {
            from: release_status_value(&deployment.release_status).to_owned(),
            to: release_status_value(&to).to_owned(),
        });
    }
    validate_release_readiness(
        &deployment.release_status,
        &to,
        &deployment.build_status,
        &deployment.serve_status,
    )?;

    let deployment_id = deployment.id;
    let to_value = release_status_value(&to);
    let mut active: deployment::ActiveModel = deployment.into();
    active.release_status = Set(to.clone());
    let updated = active.update(db).await?;

    append_event(
        db,
        deployment_id,
        DeploymentEventKind::Release,
        &format!("release status changed to {to_value}"),
        metadata,
    )
    .await
    .map_err(|error| DeploymentStateError::Database(sea_orm::DbErr::Custom(error.to_string())))?;

    Ok(updated)
}

/// Makes a deployment the single active deployment of its project
/// environment: demotes the current active one, promotes the target, and
/// records the release timeline entry. Must run inside a transaction.
pub async fn activate<C: ConnectionTrait>(
    db: &C,
    target: deployment::Model,
    reason: ReleaseReason,
    actor_user_id: Option<Uuid>,
) -> Result<deployment::Model, DeploymentStateError> {
    validate_release_readiness(
        &target.release_status,
        &DeploymentReleaseStatus::Active,
        &target.build_status,
        &target.serve_status,
    )?;

    let previous = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(target.project_id))
        .filter(deployment::Column::Environment.eq(target.environment.clone()))
        .filter(deployment::Column::ReleaseStatus.eq(DeploymentReleaseStatus::Active))
        .filter(deployment::Column::Id.ne(target.id))
        .filter(deployment::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    let previous_id = match previous {
        Some(previous) => {
            let demoted = transition_release(
                db,
                previous,
                DeploymentReleaseStatus::Approved,
                serde_json::json!({ "demoted_by": target.id }),
            )
            .await?;
            Some(demoted.id)
        }
        None => None,
    };

    let project_id = target.project_id;
    let environment = target.environment.clone();
    let activated = transition_release(
        db,
        target,
        DeploymentReleaseStatus::Active,
        serde_json::json!({
            "reason": release_reason_value(&reason),
            "previous_deployment_id": previous_id,
        }),
    )
    .await?;

    release::ActiveModel {
        id: Set(Uuid::now_v7()),
        project_id: Set(project_id),
        deployment_id: Set(activated.id),
        environment: Set(environment),
        reason: Set(reason),
        actor_user_id: Set(actor_user_id),
        previous_deployment_id: Set(previous_id),
        created_at: Set(OffsetDateTime::now_utc()),
    }
    .insert(db)
    .await?;

    Ok(activated)
}

pub fn release_reason_value(reason: &ReleaseReason) -> &'static str {
    match reason {
        ReleaseReason::Auto => "auto",
        ReleaseReason::Promote => "promote",
        ReleaseReason::Rollback => "rollback",
    }
}

#[allow(dead_code)] // Wired by the deployment timeline in Milestone 11.
pub async fn list_releases_for_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> anyhow::Result<Vec<release::Model>> {
    release::Entity::find()
        .filter(release::Column::ProjectId.eq(project_id))
        .order_by_desc(release::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

/// Whether this deployment was ever active, which is what makes it a valid
/// rollback target.
pub async fn was_active<C: ConnectionTrait>(db: &C, deployment_id: Uuid) -> anyhow::Result<bool> {
    release::Entity::find()
        .filter(release::Column::DeploymentId.eq(deployment_id))
        .one(db)
        .await
        .map(|entry| entry.is_some())
        .map_err(Into::into)
}

// --- Reviews ----------------------------------------------------------------

pub async fn create_review<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> Result<deployment_review::Model, sea_orm::DbErr> {
    let now = OffsetDateTime::now_utc();
    deployment_review::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment_id),
        reviewer_user_id: Set(None),
        status: Set(DeploymentReviewStatus::Pending),
        reason: Set(None),
        requested_at: Set(now),
        reviewed_at: Set(None),
        created_at: Set(now),
    }
    .insert(db)
    .await
}

/// Moves a ready deployment into review and creates its pending review and
/// timeline entry as one connection-scoped operation. Callers that require
/// atomicity must pass a transaction.
pub async fn request_review<C: ConnectionTrait>(
    db: &C,
    deployment: deployment::Model,
    requested_by: Option<Uuid>,
) -> Result<(deployment::Model, deployment_review::Model), DeploymentStateError> {
    let deployment = transition_release(
        db,
        deployment,
        DeploymentReleaseStatus::PendingReview,
        serde_json::json!({ "requested_by": requested_by }),
    )
    .await?;
    let review = create_review(db, deployment.id).await?;
    append_event(
        db,
        deployment.id,
        DeploymentEventKind::Review,
        "review requested",
        serde_json::json!({ "review_id": review.id }),
    )
    .await
    .map_err(|error| DeploymentStateError::Database(sea_orm::DbErr::Custom(error.to_string())))?;
    Ok((deployment, review))
}

pub async fn latest_pending_review<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<Option<deployment_review::Model>> {
    deployment_review::Entity::find()
        .filter(deployment_review::Column::DeploymentId.eq(deployment_id))
        .filter(deployment_review::Column::Status.eq(DeploymentReviewStatus::Pending))
        .order_by_desc(deployment_review::Column::RequestedAt)
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn resolve_review<C: ConnectionTrait>(
    db: &C,
    review: deployment_review::Model,
    reviewer_user_id: Uuid,
    approved: bool,
    reason: Option<String>,
) -> anyhow::Result<deployment_review::Model> {
    let mut active: deployment_review::ActiveModel = review.into();
    active.reviewer_user_id = Set(Some(reviewer_user_id));
    active.status = Set(if approved {
        DeploymentReviewStatus::Approved
    } else {
        DeploymentReviewStatus::Rejected
    });
    active.reason = Set(reason);
    active.reviewed_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await.map_err(Into::into)
}

pub async fn list_reviews<C: ConnectionTrait>(
    db: &C,
    deployment_id: Uuid,
) -> anyhow::Result<Vec<deployment_review::Model>> {
    deployment_review::Entity::find()
        .filter(deployment_review::Column::DeploymentId.eq(deployment_id))
        .order_by_desc(deployment_review::Column::RequestedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

// --- Review policy ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    /// Activation requires an approved review.
    Manual,
    /// Activation happens without review.
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyReleaseAction {
    Activate,
    RequestReview,
    None,
}

pub fn ready_release_action(
    mode: ReviewMode,
    status: &DeploymentReleaseStatus,
) -> ReadyReleaseAction {
    if !matches!(status, DeploymentReleaseStatus::Draft) {
        return ReadyReleaseAction::None;
    }
    match mode {
        ReviewMode::Manual => ReadyReleaseAction::RequestReview,
        ReviewMode::Auto => ReadyReleaseAction::None,
    }
}

pub fn serve_ready_release_action(
    mode: ReviewMode,
    status: &DeploymentReleaseStatus,
) -> ReadyReleaseAction {
    match (mode, status) {
        (ReviewMode::Auto, DeploymentReleaseStatus::Draft) => ReadyReleaseAction::Activate,
        _ => ReadyReleaseAction::None,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ReviewPolicy {
    pub production: ReviewMode,
    pub preview: ReviewMode,
}

impl ReviewPolicy {
    pub fn mode_for(&self, environment: &DeploymentEnvironment) -> ReviewMode {
        match environment {
            DeploymentEnvironment::Production => self.production,
            DeploymentEnvironment::Preview => self.preview,
        }
    }
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        Self {
            production: ReviewMode::Manual,
            preview: ReviewMode::Auto,
        }
    }
}

fn parse_mode(value: Option<&serde_json::Value>, default: ReviewMode) -> ReviewMode {
    match value.and_then(|value| value.as_str()) {
        Some("auto") => ReviewMode::Auto,
        Some("manual") => ReviewMode::Manual,
        _ => default,
    }
}

/// Reads the system release review policy seeded as
/// `release_review_policy.default`.
pub async fn review_policy<C: ConnectionTrait>(db: &C) -> anyhow::Result<ReviewPolicy> {
    let setting = crate::domain::settings::get_setting(db, "release_review_policy.default").await?;
    let defaults = ReviewPolicy::default();
    Ok(match setting {
        Some(setting) => ReviewPolicy {
            production: parse_mode(setting.value.get("production"), defaults.production),
            preview: parse_mode(setting.value.get("preview"), defaults.preview),
        },
        None => defaults,
    })
}

fn apply_review_policy_override(
    defaults: ReviewPolicy,
    policy: Option<&serde_json::Value>,
) -> ReviewPolicy {
    ReviewPolicy {
        production: parse_mode(
            policy.and_then(|policy| policy.get("production")),
            defaults.production,
        ),
        preview: parse_mode(
            policy.and_then(|policy| policy.get("preview")),
            defaults.preview,
        ),
    }
}

/// Resolves each environment as Team Group override > platform default.
pub async fn review_policy_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
) -> anyhow::Result<ReviewPolicy> {
    let defaults = review_policy(db).await?;
    let team = team::Entity::find_by_id(team_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("team not found while resolving review policy"))?;
    let Some(group_id) = team.group_id else {
        return Ok(defaults);
    };
    let group = team_group::Entity::find_by_id(group_id)
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await?;

    Ok(apply_review_policy_override(
        defaults,
        group
            .as_ref()
            .and_then(|group| group.review_policy.as_ref()),
    ))
}

/// Whether a runtime kind is deployable. Returns the stable failure message
/// for the kinds that are still unimplemented (hybrid/serverless/edge).
pub fn runtime_failure(runtime: &ProjectRuntime) -> Option<(&'static str, &'static str)> {
    match runtime {
        ProjectRuntime::Static | ProjectRuntime::Ssr => None,
        ProjectRuntime::Hybrid => Some((
            "runtime_not_implemented",
            "Hybrid runtime is not implemented yet",
        )),
        ProjectRuntime::Serverless => Some((
            "runtime_not_implemented",
            "Serverless runtime is not implemented yet",
        )),
        ProjectRuntime::Edge => Some((
            "runtime_not_implemented",
            "Edge runtime is not implemented yet",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_transitions_follow_the_state_machine() {
        use DeploymentBuildStatus as B;
        assert!(can_transition_build(&B::Pending, &B::Claimed));
        assert!(can_transition_build(&B::Claimed, &B::Queued));
        assert!(can_transition_build(&B::Claimed, &B::Building));
        assert!(can_transition_build(&B::Queued, &B::Building));
        assert!(can_transition_build(&B::Building, &B::Ready));
        assert!(can_transition_build(&B::Building, &B::Failed));
        assert!(can_transition_build(&B::Building, &B::Canceled));
        assert!(can_transition_build(&B::Pending, &B::Canceled));

        assert!(!can_transition_build(&B::Ready, &B::Building));
        assert!(!can_transition_build(&B::Failed, &B::Ready));
        assert!(!can_transition_build(&B::Canceled, &B::Building));
        assert!(!can_transition_build(&B::Pending, &B::Building));
        assert!(!can_transition_build(&B::Ready, &B::Canceled));
    }

    #[test]
    fn release_transitions_follow_the_state_machine() {
        use DeploymentReleaseStatus as R;
        assert!(can_transition_release(&R::Draft, &R::PendingReview));
        assert!(can_transition_release(&R::Draft, &R::Active));
        assert!(can_transition_release(&R::PendingReview, &R::Approved));
        assert!(can_transition_release(&R::PendingReview, &R::Rejected));
        assert!(can_transition_release(&R::Rejected, &R::PendingReview));
        assert!(can_transition_release(&R::Approved, &R::Active));
        assert!(can_transition_release(&R::Active, &R::Approved));
        assert!(can_transition_release(&R::Active, &R::Draft));

        assert!(!can_transition_release(&R::Rejected, &R::Active));
        assert!(!can_transition_release(&R::Draft, &R::Approved));
        assert!(!can_transition_release(&R::Approved, &R::Rejected));
    }

    #[test]
    fn release_readiness_requires_build_and_serve() {
        use DeploymentBuildStatus as B;
        use DeploymentReleaseStatus as R;
        use DeploymentServeStatus as S;

        assert!(validate_release_readiness(&R::Draft, &R::Active, &B::Ready, &S::Ready).is_ok());
        assert!(
            validate_release_readiness(&R::Draft, &R::PendingReview, &B::Ready, &S::Syncing)
                .is_ok()
        );
        assert!(matches!(
            validate_release_readiness(&R::Draft, &R::Active, &B::Ready, &S::Pending),
            Err(DeploymentStateError::ServeNotReady)
        ));
        assert!(matches!(
            validate_release_readiness(&R::PendingReview, &R::Approved, &B::Ready, &S::Syncing),
            Err(DeploymentStateError::ServeNotReady)
        ));
        assert!(
            validate_release_readiness(&R::PendingReview, &R::Approved, &B::Ready, &S::Retired)
                .is_ok()
        );
        assert!(matches!(
            validate_release_readiness(&R::Draft, &R::Active, &B::Building, &S::Ready),
            Err(DeploymentStateError::BuildNotReady)
        ));
        assert!(
            validate_release_readiness(&R::Active, &R::Approved, &B::Ready, &S::Syncing).is_ok()
        );
    }

    #[test]
    fn serve_transitions_require_sync_and_allow_failed_recovery() {
        use DeploymentServeStatus as S;

        assert!(can_transition_serve(&S::Pending, &S::Syncing));
        assert!(can_transition_serve(&S::Syncing, &S::Ready));
        assert!(can_transition_serve(&S::Syncing, &S::Failed));
        assert!(can_transition_serve(&S::Failed, &S::Syncing));
        assert!(can_transition_serve(&S::Ready, &S::Syncing));
        assert!(can_transition_serve(&S::Pending, &S::Retired));
        assert!(can_transition_serve(&S::Syncing, &S::Retired));
        assert!(can_transition_serve(&S::Failed, &S::Retired));
        assert!(can_transition_serve(&S::Ready, &S::Retired));
        assert!(can_transition_serve(&S::Retired, &S::Pending));
        assert_eq!(serve_status_value(&S::Retired), "retired");
        assert!(!can_transition_serve(&S::Pending, &S::Ready));
        assert!(!can_transition_serve(&S::Failed, &S::Ready));
        assert!(!can_transition_serve(&S::Retired, &S::Ready));
    }

    #[test]
    fn static_and_ssr_runtimes_are_deployable() {
        assert!(runtime_failure(&ProjectRuntime::Static).is_none());
        assert!(runtime_failure(&ProjectRuntime::Ssr).is_none());
        for runtime in [
            ProjectRuntime::Hybrid,
            ProjectRuntime::Serverless,
            ProjectRuntime::Edge,
        ] {
            let (code, message) = runtime_failure(&runtime).expect("must fail");
            assert_eq!(code, "runtime_not_implemented");
            assert!(message.contains("not implemented"));
        }
    }

    #[test]
    fn review_policy_defaults_to_manual_production_auto_preview() {
        let policy = ReviewPolicy::default();
        assert_eq!(
            policy.mode_for(&DeploymentEnvironment::Production),
            ReviewMode::Manual
        );
        assert_eq!(
            policy.mode_for(&DeploymentEnvironment::Preview),
            ReviewMode::Auto
        );
    }

    #[test]
    fn team_group_review_policy_overrides_each_environment_independently() {
        let defaults = ReviewPolicy::default();
        let production_override = serde_json::json!({ "production": "auto" });
        let policy = apply_review_policy_override(defaults, Some(&production_override));

        assert!(matches!(policy.production, ReviewMode::Auto));
        assert!(matches!(policy.preview, ReviewMode::Auto));

        let preview_override = serde_json::json!({ "preview": "manual" });
        let policy = apply_review_policy_override(defaults, Some(&preview_override));
        assert!(matches!(policy.production, ReviewMode::Manual));
        assert!(matches!(policy.preview, ReviewMode::Manual));
    }

    #[test]
    fn build_ready_drafts_only_create_manual_reviews() {
        assert_eq!(
            ready_release_action(ReviewMode::Manual, &DeploymentReleaseStatus::Draft),
            ReadyReleaseAction::RequestReview
        );
        assert_eq!(
            ready_release_action(ReviewMode::Auto, &DeploymentReleaseStatus::Draft),
            ReadyReleaseAction::None
        );
    }

    #[test]
    fn serve_ready_drafts_activate_only_under_auto_policy() {
        assert_eq!(
            serve_ready_release_action(ReviewMode::Auto, &DeploymentReleaseStatus::Draft),
            ReadyReleaseAction::Activate
        );
        assert_eq!(
            serve_ready_release_action(ReviewMode::Manual, &DeploymentReleaseStatus::Draft),
            ReadyReleaseAction::None
        );
    }

    #[test]
    fn ready_finalization_is_idempotent_after_the_draft_state() {
        for status in [
            DeploymentReleaseStatus::PendingReview,
            DeploymentReleaseStatus::Approved,
            DeploymentReleaseStatus::Rejected,
            DeploymentReleaseStatus::Active,
        ] {
            assert_eq!(
                ready_release_action(ReviewMode::Manual, &status),
                ReadyReleaseAction::None
            );
            assert_eq!(
                ready_release_action(ReviewMode::Auto, &status),
                ReadyReleaseAction::None
            );
        }
    }
}
