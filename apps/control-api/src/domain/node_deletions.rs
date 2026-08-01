use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments, nodes, scheduler,
    },
    infra::database::entity::{
        AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus, DeploymentServeStatus,
        NodeDeletionStatus, NodeDeploymentMigrationStatus, NodeStatus, deployment,
        deployment_artifact, node, node_deletion_job, node_deployment_migration,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionPhase {
    Migrating,
    Draining,
    Deleting,
    Failed,
}

pub fn next_phase(
    total_migrations: u64,
    ready_migrations: u64,
    failed_migrations: u64,
    active_builds: u64,
) -> DeletionPhase {
    if failed_migrations > 0 {
        DeletionPhase::Failed
    } else if ready_migrations < total_migrations {
        DeletionPhase::Migrating
    } else if active_builds > 0 {
        DeletionPhase::Draining
    } else {
        DeletionPhase::Deleting
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EligibleTarget {
    pub id: Uuid,
    pub name: String,
    pub available_deployments: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletionPlan {
    pub node_id: Uuid,
    pub assigned_deployments: u64,
    pub active_builds: u64,
    pub requires_target: bool,
    pub eligible_targets: Vec<EligibleTarget>,
}

fn assigned_filter(node_id: Uuid) -> sea_orm::Condition {
    sea_orm::Condition::all()
        .add(deployment::Column::ServeNodeId.eq(node_id))
        .add(deployment::Column::DeletedAt.is_null())
        .add(deployment::Column::BuildStatus.ne(DeploymentBuildStatus::Failed))
        .add(deployment::Column::BuildStatus.ne(DeploymentBuildStatus::Canceled))
        .add(deployment::Column::ServeStatus.ne(DeploymentServeStatus::Retired))
}

async fn assigned_deployments<C: ConnectionTrait>(
    db: &C,
    node_id: Uuid,
) -> anyhow::Result<Vec<deployment::Model>> {
    deployment::Entity::find()
        .filter(assigned_filter(node_id))
        .order_by_asc(deployment::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn active_build_count<C: ConnectionTrait>(db: &C, node_id: Uuid) -> anyhow::Result<u64> {
    deployment::Entity::find()
        .filter(deployment::Column::BuildNodeId.eq(node_id))
        .filter(deployment::Column::BuildStatus.is_in([
            DeploymentBuildStatus::Claimed,
            DeploymentBuildStatus::Queued,
            DeploymentBuildStatus::Building,
        ]))
        .filter(deployment::Column::DeletedAt.is_null())
        .count(db)
        .await
        .map_err(Into::into)
}

fn candidate_can_host(candidate: &scheduler::Candidate, deployments: &[deployment::Model]) -> bool {
    let requested_cpu = deployments.iter().try_fold(0_u64, |total, deployment| {
        u64::try_from(deployment.serve_cpu_millicores)
            .ok()
            .and_then(|value| total.checked_add(value))
    });
    let requested_memory = deployments.iter().try_fold(0_u64, |total, deployment| {
        u64::try_from(deployment.serve_memory_mb)
            .ok()
            .and_then(|value| total.checked_add(value))
    });
    let requested_disk = deployments.iter().try_fold(0_u64, |total, deployment| {
        u64::try_from(deployment.serve_disk_mb)
            .ok()
            .and_then(|value| total.checked_add(value))
    });
    let Some((requested_cpu, requested_memory, requested_disk)) = requested_cpu
        .zip(requested_memory)
        .zip(requested_disk)
        .map(|((cpu, memory), disk)| (cpu, memory, disk))
    else {
        return false;
    };
    candidate
        .usage
        .cpu_millicores
        .checked_add(requested_cpu)
        .is_some_and(|value| value <= candidate.capacity.cpu_millicores)
        && candidate
            .usage
            .memory_mb
            .checked_add(requested_memory)
            .is_some_and(|value| value <= candidate.capacity.memory_mb)
        && candidate
            .usage
            .disk_mb
            .checked_add(requested_disk)
            .is_some_and(|value| value <= candidate.capacity.disk_mb)
        && candidate
            .usage
            .deployments
            .checked_add(deployments.len() as u64)
            .is_some_and(|value| value <= u64::from(candidate.capacity.max_deployments))
}

pub async fn plan<C: ConnectionTrait>(
    db: &C,
    source: &node::Model,
) -> anyhow::Result<DeletionPlan> {
    let deployments = assigned_deployments(db, source.id).await?;
    let active_builds = active_build_count(db, source.id).await?;
    let candidates = scheduler::eligible_candidates(db).await?;
    let candidate_ids = candidates
        .iter()
        .filter(|candidate| candidate.node_id != source.id)
        .filter(|candidate| candidate_can_host(candidate, &deployments))
        .map(|candidate| candidate.node_id)
        .collect::<Vec<_>>();
    let names = if candidate_ids.is_empty() {
        Vec::new()
    } else {
        node::Entity::find()
            .filter(node::Column::Id.is_in(candidate_ids))
            .all(db)
            .await?
    };
    let usage = candidates
        .into_iter()
        .map(|candidate| (candidate.node_id, candidate))
        .collect::<std::collections::HashMap<_, _>>();
    let mut eligible_targets = names
        .into_iter()
        .filter_map(|target| {
            let candidate = usage.get(&target.id)?;
            Some(EligibleTarget {
                id: target.id,
                name: target.name,
                available_deployments: u64::from(candidate.capacity.max_deployments)
                    .saturating_sub(candidate.usage.deployments),
            })
        })
        .collect::<Vec<_>>();
    eligible_targets.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(DeletionPlan {
        node_id: source.id,
        assigned_deployments: deployments.len() as u64,
        active_builds,
        requires_target: !deployments.is_empty(),
        eligible_targets,
    })
}

pub async fn active_job<C: ConnectionTrait>(
    db: &C,
    node_id: Uuid,
) -> anyhow::Result<Option<node_deletion_job::Model>> {
    node_deletion_job::Entity::find()
        .filter(node_deletion_job::Column::NodeId.eq(node_id))
        .filter(node_deletion_job::Column::Status.ne(NodeDeletionStatus::Completed))
        .order_by_desc(node_deletion_job::Column::CreatedAt)
        .one(db)
        .await
        .map_err(Into::into)
}

pub fn status_value(status: &NodeDeletionStatus) -> &'static str {
    match status {
        NodeDeletionStatus::Queued => "queued",
        NodeDeletionStatus::Migrating => "migrating",
        NodeDeletionStatus::Draining => "draining",
        NodeDeletionStatus::Deleting => "deleting",
        NodeDeletionStatus::Failed => "failed",
        NodeDeletionStatus::Completed => "completed",
    }
}

async fn create_job_audit(
    transaction: &sea_orm::DatabaseTransaction,
    job: &node_deletion_job::Model,
    action: &str,
    result: AuditEventResult,
    reason: Option<String>,
    metadata: serde_json::Value,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
        transaction,
        CreateAuditEventParams {
            actor_user_id: job.requested_by_user_id,
            actor_node_id: None,
            team_id: None,
            action: action.to_owned(),
            target_type: "node".to_owned(),
            target_id: Some(job.node_id),
            result,
            reason,
            metadata,
        },
    )
    .await
}

fn bounded_job_error(error: impl std::fmt::Display) -> String {
    error.to_string().chars().take(2_048).collect()
}

pub async fn enqueue(
    transaction: &sea_orm::DatabaseTransaction,
    source: node::Model,
    target_node_id: Option<Uuid>,
    requested_by_user_id: Uuid,
) -> anyhow::Result<node_deletion_job::Model> {
    let plan = plan(transaction, &source).await?;
    if plan.requires_target {
        let target =
            target_node_id.ok_or_else(|| anyhow::anyhow!("replacement Serve Node required"))?;
        if !plan
            .eligible_targets
            .iter()
            .any(|candidate| candidate.id == target)
        {
            anyhow::bail!("selected replacement Serve Node is unavailable or lacks capacity");
        }
    }
    let deployments = assigned_deployments(transaction, source.id).await?;
    let now = OffsetDateTime::now_utc();
    let existing = active_job(transaction, source.id).await?;
    let retrying = existing.is_some();
    let job = if let Some(job) = existing {
        if !matches!(job.status, NodeDeletionStatus::Failed) {
            anyhow::bail!("node deletion is already in progress");
        }
        node_deployment_migration::Entity::delete_many()
            .filter(node_deployment_migration::Column::JobId.eq(job.id))
            .exec(transaction)
            .await?;
        let mut active: node_deletion_job::ActiveModel = job.into();
        active.target_node_id = Set(target_node_id);
        active.status = Set(NodeDeletionStatus::Queued);
        active.total_deployments = Set(i32::try_from(deployments.len())?);
        active.migrated_deployments = Set(0);
        active.active_builds = Set(i32::try_from(plan.active_builds)?);
        active.error = Set(None);
        active.updated_at = Set(now);
        active.update(transaction).await?
    } else {
        node_deletion_job::ActiveModel {
            id: Set(Uuid::now_v7()),
            node_id: Set(source.id),
            target_node_id: Set(target_node_id),
            requested_by_user_id: Set(Some(requested_by_user_id)),
            status: Set(NodeDeletionStatus::Queued),
            total_deployments: Set(i32::try_from(deployments.len())?),
            migrated_deployments: Set(0),
            active_builds: Set(i32::try_from(plan.active_builds)?),
            error: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            completed_at: Set(None),
        }
        .insert(transaction)
        .await?
    };
    if let Some(target_node_id) = target_node_id
        && !deployments.is_empty()
    {
        node_deployment_migration::Entity::insert_many(deployments.into_iter().map(|deployment| {
            node_deployment_migration::ActiveModel {
                id: Set(Uuid::now_v7()),
                job_id: Set(job.id),
                deployment_id: Set(deployment.id),
                source_node_id: Set(source.id),
                target_node_id: Set(target_node_id),
                status: Set(NodeDeploymentMigrationStatus::Pending),
                error: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
                ready_at: Set(None),
            }
        }))
        .exec(transaction)
        .await?;
    }
    let mut active: node::ActiveModel = source.into();
    active.status = Set(NodeStatus::Draining);
    active.update(transaction).await?;
    create_job_audit(
        transaction,
        &job,
        if retrying {
            "node.deletion_retried"
        } else {
            "node.deletion_queued"
        },
        AuditEventResult::Success,
        None,
        serde_json::json!({
            "job_id": job.id,
            "target_node_id": job.target_node_id,
            "deployments": job.total_deployments,
            "active_builds": job.active_builds,
        }),
    )
    .await?;
    Ok(job)
}

async fn update_job_phase(
    transaction: &sea_orm::DatabaseTransaction,
    job: node_deletion_job::Model,
    status: NodeDeletionStatus,
    ready: u64,
    active_builds: u64,
    error: Option<String>,
) -> anyhow::Result<node_deletion_job::Model> {
    let mut active: node_deletion_job::ActiveModel = job.into();
    active.status = Set(status);
    active.migrated_deployments = Set(i32::try_from(ready)?);
    active.active_builds = Set(i32::try_from(active_builds)?);
    active.error = Set(error);
    active.updated_at = Set(OffsetDateTime::now_utc());
    active.update(transaction).await.map_err(Into::into)
}

async fn fail_job(
    transaction: &sea_orm::DatabaseTransaction,
    job: node_deletion_job::Model,
    ready: u64,
    active_builds: u64,
    message: String,
    stage: &str,
) -> anyhow::Result<node_deletion_job::Model> {
    let message = bounded_job_error(message);
    let now = OffsetDateTime::now_utc();
    node_deployment_migration::Entity::update_many()
        .col_expr(
            node_deployment_migration::Column::Status,
            sea_orm::ActiveEnum::as_enum(&NodeDeploymentMigrationStatus::Failed),
        )
        .col_expr(
            node_deployment_migration::Column::Error,
            sea_orm::sea_query::Expr::value(Some(message.clone())),
        )
        .col_expr(
            node_deployment_migration::Column::ReadyAt,
            sea_orm::sea_query::Expr::value(Option::<OffsetDateTime>::None),
        )
        .col_expr(
            node_deployment_migration::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(node_deployment_migration::Column::JobId.eq(job.id))
        .filter(node_deployment_migration::Column::Status.ne(NodeDeploymentMigrationStatus::Failed))
        .exec(transaction)
        .await?;
    let failed = update_job_phase(
        transaction,
        job,
        NodeDeletionStatus::Failed,
        ready,
        active_builds,
        Some(message.clone()),
    )
    .await?;
    create_job_audit(
        transaction,
        &failed,
        "node.deletion_failed",
        AuditEventResult::Failure,
        Some(message),
        serde_json::json!({
            "job_id": failed.id,
            "target_node_id": failed.target_node_id,
            "migrated_deployments": ready,
            "total_deployments": failed.total_deployments,
            "active_builds": active_builds,
            "stage": stage,
        }),
    )
    .await?;
    Ok(failed)
}

fn artifact_validation_error(artifact: &deployment_artifact::Model) -> Option<&'static str> {
    let checksum_valid = artifact.checksum_sha256.as_deref().is_some_and(|checksum| {
        checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    if !checksum_valid {
        return Some("Grass Output artifact checksum metadata is missing or invalid");
    }
    if artifact.size_bytes.is_none_or(|size| size < 0) {
        return Some("Grass Output artifact size metadata is missing or invalid");
    }
    if deployments::artifact_unpacked_size_bytes(&artifact.manifest).is_none() {
        return Some("Grass Output artifact unpacked size metadata is invalid");
    }
    None
}

fn target_is_available(target: &node::Model, now: OffsetDateTime) -> bool {
    target.serve_enabled
        && target
            .base_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty())
        && nodes::is_healthy(target, now, 90)
}

async fn switch_ready_routes(
    transaction: &sea_orm::DatabaseTransaction,
    job: &node_deletion_job::Model,
    migrations: &[node_deployment_migration::Model],
) -> anyhow::Result<u64> {
    let Some(target_node_id) = job.target_node_id else {
        return Ok(0);
    };
    let ids = migrations
        .iter()
        .map(|migration| migration.deployment_id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(0);
    }
    let updated = deployment::Entity::update_many()
        .col_expr(
            deployment::Column::ServeNodeId,
            sea_orm::sea_query::Expr::value(Some(target_node_id)),
        )
        .col_expr(
            deployment::Column::Overcommitted,
            sea_orm::sea_query::Expr::value(false),
        )
        .filter(deployment::Column::Id.is_in(ids))
        .filter(deployment::Column::ServeNodeId.eq(job.node_id))
        .filter(deployment::Column::DeletedAt.is_null())
        .filter(deployment::Column::BuildStatus.ne(DeploymentBuildStatus::Failed))
        .filter(deployment::Column::BuildStatus.ne(DeploymentBuildStatus::Canceled))
        .filter(deployment::Column::ServeStatus.ne(DeploymentServeStatus::Retired))
        .exec(transaction)
        .await?;
    create_job_audit(
        transaction,
        job,
        "node.deletion_route_switched",
        AuditEventResult::Success,
        None,
        serde_json::json!({
            "job_id": job.id,
            "source_node_id": job.node_id,
            "target_node_id": target_node_id,
            "ready_deployments": migrations.len(),
            "routes_updated": updated.rows_affected,
        }),
    )
    .await?;
    Ok(updated.rows_affected)
}

async fn process_job(db: &DatabaseConnection, job_id: Uuid) -> anyhow::Result<()> {
    let transaction = db.begin().await?;
    scheduler::lock_placement(&transaction).await?;
    let Some(job) = node_deletion_job::Entity::find_by_id(job_id)
        .lock_exclusive()
        .one(&transaction)
        .await?
    else {
        return Ok(());
    };
    if matches!(
        job.status,
        NodeDeletionStatus::Failed | NodeDeletionStatus::Completed
    ) {
        return Ok(());
    }
    let migrations = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::JobId.eq(job.id))
        .all(&transaction)
        .await?;
    if job.migrated_deployments < job.total_deployments {
        let current_ready = migrations
            .iter()
            .filter(|migration| matches!(migration.status, NodeDeploymentMigrationStatus::Ready))
            .count() as u64;
        let target_available = match job.target_node_id {
            Some(target_node_id) => node::Entity::find_by_id(target_node_id)
                .lock_exclusive()
                .one(&transaction)
                .await?
                .is_some_and(|target| target_is_available(&target, OffsetDateTime::now_utc())),
            None => migrations.is_empty(),
        };
        if !target_available {
            let active_builds = active_build_count(&transaction, job.node_id).await?;
            fail_job(
                &transaction,
                job,
                current_ready,
                active_builds,
                "replacement Serve Node is no longer active and healthy".to_owned(),
                "target_validation",
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
    }
    // Terminal or retired deployments no longer need a shadow copy.
    for migration in &migrations {
        if matches!(migration.status, NodeDeploymentMigrationStatus::Ready) {
            continue;
        }
        let deployment = deployment::Entity::find_by_id(migration.deployment_id)
            .one(&transaction)
            .await?;
        if deployment.as_ref().is_none_or(|deployment| {
            deployment.deleted_at.is_some()
                || matches!(
                    deployment.build_status,
                    DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
                )
                || matches!(deployment.serve_status, DeploymentServeStatus::Retired)
        }) {
            let mut active: node_deployment_migration::ActiveModel = migration.clone().into();
            active.status = Set(NodeDeploymentMigrationStatus::Ready);
            active.error = Set(None);
            active.ready_at = Set(Some(OffsetDateTime::now_utc()));
            active.updated_at = Set(OffsetDateTime::now_utc());
            active.update(&transaction).await?;
        } else if deployment.as_ref().is_some_and(|deployment| {
            matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        }) {
            let artifact = deployment_artifact::Entity::find()
                .filter(deployment_artifact::Column::DeploymentId.eq(migration.deployment_id))
                .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
                .filter(deployment_artifact::Column::DeletedAt.is_null())
                .one(&transaction)
                .await?;
            let artifact_error = match artifact.as_ref() {
                Some(artifact) => artifact_validation_error(artifact),
                None => Some("Grass Output artifact is missing"),
            };
            if let Some(error) = artifact_error {
                let mut active: node_deployment_migration::ActiveModel = migration.clone().into();
                active.status = Set(NodeDeploymentMigrationStatus::Failed);
                active.error = Set(Some(error.to_owned()));
                active.ready_at = Set(None);
                active.updated_at = Set(OffsetDateTime::now_utc());
                active.update(&transaction).await?;
            }
        }
    }
    let migrations = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::JobId.eq(job.id))
        .all(&transaction)
        .await?;
    let ready = migrations
        .iter()
        .filter(|migration| matches!(migration.status, NodeDeploymentMigrationStatus::Ready))
        .count() as u64;
    let failed = migrations
        .iter()
        .filter(|migration| matches!(migration.status, NodeDeploymentMigrationStatus::Failed))
        .count() as u64;
    let active_builds = active_build_count(&transaction, job.node_id).await?;
    match next_phase(migrations.len() as u64, ready, failed, active_builds) {
        DeletionPhase::Failed => {
            let message = migrations
                .iter()
                .find_map(|migration| migration.error.clone())
                .unwrap_or_else(|| "deployment migration failed".to_owned());
            fail_job(
                &transaction,
                job,
                ready,
                active_builds,
                message,
                "shadow_migration",
            )
            .await?;
        }
        DeletionPhase::Migrating => {
            update_job_phase(
                &transaction,
                job,
                NodeDeletionStatus::Migrating,
                ready,
                active_builds,
                None,
            )
            .await?;
        }
        DeletionPhase::Draining => {
            if ready == migrations.len() as u64 && job.migrated_deployments < job.total_deployments
            {
                switch_ready_routes(&transaction, &job, &migrations).await?;
            }
            update_job_phase(
                &transaction,
                job,
                NodeDeletionStatus::Draining,
                ready,
                active_builds,
                None,
            )
            .await?;
        }
        DeletionPhase::Deleting => {
            if job.migrated_deployments < job.total_deployments {
                switch_ready_routes(&transaction, &job, &migrations).await?;
            }
            if !matches!(job.status, NodeDeletionStatus::Deleting) {
                update_job_phase(
                    &transaction,
                    job,
                    NodeDeletionStatus::Deleting,
                    ready,
                    0,
                    None,
                )
                .await?;
            } else {
                let now = OffsetDateTime::now_utc();
                let Some(source) = node::Entity::find_by_id(job.node_id)
                    .lock_exclusive()
                    .one(&transaction)
                    .await?
                else {
                    return Ok(());
                };
                let mut active: node::ActiveModel = source.into();
                active.status = Set(NodeStatus::Disabled);
                active.deleted_at = Set(Some(now));
                active.update(&transaction).await?;
                let mut active: node_deletion_job::ActiveModel = job.clone().into();
                active.status = Set(NodeDeletionStatus::Completed);
                active.migrated_deployments = Set(i32::try_from(ready)?);
                active.active_builds = Set(0);
                active.error = Set(None);
                active.updated_at = Set(now);
                active.completed_at = Set(Some(now));
                active.update(&transaction).await?;
                audits::create_platform_audit_event(
                    &transaction,
                    CreateAuditEventParams {
                        actor_user_id: job.requested_by_user_id,
                        actor_node_id: None,
                        team_id: None,
                        action: "node.deleted".to_owned(),
                        target_type: "node".to_owned(),
                        target_id: Some(job.node_id),
                        result: AuditEventResult::Success,
                        reason: None,
                        metadata: serde_json::json!({
                            "job_id": job.id,
                            "target_node_id": job.target_node_id,
                            "migrated_deployments": ready,
                        }),
                    },
                )
                .await?;
            }
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn process_pending_jobs(db: &DatabaseConnection) -> anyhow::Result<u64> {
    let jobs = node_deletion_job::Entity::find()
        .filter(node_deletion_job::Column::Status.is_in([
            NodeDeletionStatus::Queued,
            NodeDeletionStatus::Migrating,
            NodeDeletionStatus::Draining,
            NodeDeletionStatus::Deleting,
        ]))
        .order_by_asc(node_deletion_job::Column::UpdatedAt)
        .all(db)
        .await?;
    let mut processed = 0;
    for job in jobs {
        if let Err(error) = process_job(db, job.id).await {
            tracing::error!(
                operation = "node_deletions.process_job",
                job_id = %job.id,
                node_id = %job.node_id,
                %error,
                "node deletion job failed"
            );
            let transaction = db.begin().await?;
            if let Some(current) = node_deletion_job::Entity::find_by_id(job.id)
                .lock_exclusive()
                .one(&transaction)
                .await?
                .filter(|current| {
                    !matches!(
                        current.status,
                        NodeDeletionStatus::Failed | NodeDeletionStatus::Completed
                    )
                })
            {
                fail_job(
                    &transaction,
                    current,
                    job.migrated_deployments.max(0) as u64,
                    job.active_builds.max(0) as u64,
                    bounded_job_error(error),
                    "queue_processing",
                )
                .await?;
            }
            transaction.commit().await?;
        }
        processed += 1;
    }
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use axum::{
        Extension, Json,
        body::to_bytes,
        extract::{Path, State},
        response::IntoResponse,
    };
    use grass_cache::{CacheBackend, CacheStore};
    use grass_node_protocol::{ClaimRequest, ReportServeStatusRequest, ReportedServeStatus};
    use sea_orm::{
        ActiveModelTrait, ActiveValue::Set, ColumnTrait, Database, DatabaseConnection, EntityTrait,
        QueryFilter, TransactionTrait,
    };
    use sea_orm_migration::MigratorTrait;

    use crate::{
        domain::{
            deployments::{self, CreateDeploymentParams},
            nodes,
            projects::{self, CreateProjectParams},
            scheduler::{Placement, PlacementMode},
            teams::{self, CreateTeamParams},
        },
        infra::database::{
            entity::{
                AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus,
                DeploymentEnvironment, DeploymentReleaseStatus, DeploymentServeStatus,
                NodeConfigSyncStatus, PlatformRole, ProjectRuntime, TeamKind, UserStatus,
                audit_event, deployment_artifact, project, user,
            },
            migrate::Migrator,
        },
        infra::{config::ControlApiConfig, http::middlewares::node_auth::AuthenticatedNode},
        state::ControlApiState,
    };

    use super::*;

    static MIGRATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct PostgresTestDatabase {
        db: DatabaseConnection,
        admin: DatabaseConnection,
        schema: String,
    }

    impl PostgresTestDatabase {
        async fn start() -> Option<Self> {
            let database_url = std::env::var("GRASS_TEST_DATABASE_URL").ok()?;
            let admin = Database::connect(&database_url).await.unwrap();
            let schema = format!("gw_node_deletion_{}", Uuid::now_v7().simple());
            admin
                .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
                .await
                .unwrap();
            let mut scoped_url = url::Url::parse(&database_url).unwrap();
            scoped_url
                .query_pairs_mut()
                .append_pair("options", &format!("-csearch_path={schema}"));
            let db = Database::connect(scoped_url.as_str()).await.unwrap();
            let _migration_guard = MIGRATION_LOCK.lock().await;
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

    struct DeletionFixture {
        user: user::Model,
        project: project::Model,
        source: node::Model,
        target: node::Model,
        deployment: deployment::Model,
    }

    async fn active_node(db: &DatabaseConnection, name: &str) -> node::Model {
        let now = OffsetDateTime::now_utc();
        node::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name.to_owned()),
            token_hash: Set(format!("token-{name}")),
            status: Set(NodeStatus::Active),
            build_enabled: Set(true),
            serve_enabled: Set(true),
            build_concurrency: Set(2),
            base_url: Set(Some(format!("http://{name}.example.test"))),
            work_root: Set(Some(format!("/data/{name}"))),
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
        .unwrap()
    }

    async fn create_deployment(
        db: &DatabaseConnection,
        project: &project::Model,
        source_node_id: Uuid,
        with_artifact: bool,
        active_release: bool,
    ) -> deployment::Model {
        let deployment = deployments::create_deployment(
            db,
            CreateDeploymentParams {
                project: project.clone(),
                environment: DeploymentEnvironment::Production,
                triggered_by_user_id: None,
                branch: Some("main".to_owned()),
                commit_hash: None,
                commit_message: None,
                preview_host: None,
                source_credential_version_id: None,
            },
            Placement {
                node_id: source_node_id,
                overcommitted: false,
                mode: PlacementMode::Automatic,
            },
        )
        .await
        .unwrap();
        let mut active: deployment::ActiveModel = deployment.into();
        active.build_status = Set(DeploymentBuildStatus::Ready);
        active.serve_status = Set(DeploymentServeStatus::Ready);
        active.release_status = Set(if active_release {
            DeploymentReleaseStatus::Active
        } else {
            DeploymentReleaseStatus::Draft
        });
        let deployment = active.update(db).await.unwrap();
        if with_artifact {
            deployment_artifact::ActiveModel {
                id: Set(Uuid::now_v7()),
                deployment_id: Set(deployment.id),
                kind: Set(DeploymentArtifactKind::GrassOutput),
                storage_path: Set(format!("artifacts/{}.zip", deployment.id)),
                checksum_sha256: Set(Some("a".repeat(64))),
                size_bytes: Set(Some(128)),
                manifest: Set(serde_json::json!({ "unpacked_size_bytes": 256 })),
                deleted_at: Set(None),
                created_at: Set(OffsetDateTime::now_utc()),
            }
            .insert(db)
            .await
            .unwrap();
        }
        deployment
    }

    async fn seed_fixture(db: &DatabaseConnection, with_artifact: bool) -> DeletionFixture {
        let now = OffsetDateTime::now_utc();
        let user = user::ActiveModel {
            id: Set(Uuid::now_v7()),
            email: Set(format!("{}@example.test", Uuid::now_v7().simple())),
            display_name: Set(Some("Node Deletion Tester".to_owned())),
            status: Set(UserStatus::Active),
            platform_role: Set(PlatformRole::Admin),
            last_login_at: Set(None),
            deleted_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .unwrap();
        let team = teams::create_team(
            db,
            CreateTeamParams {
                slug: format!("team-{}", Uuid::now_v7().simple()),
                name: "Node Deletion Team".to_owned(),
                kind: TeamKind::Team,
                owner_user_id: user.id,
                group_id: None,
            },
        )
        .await
        .unwrap();
        let project = projects::create_project(
            db,
            CreateProjectParams {
                team_id: team.id,
                created_by_user_id: None,
                slug: format!("project-{}", Uuid::now_v7().simple()),
                name: "Node Deletion Project".to_owned(),
                runtime: ProjectRuntime::Static,
                repository_url: None,
                default_branch: Some("main".to_owned()),
                install_command: None,
                build_command: None,
                output_directory: None,
                source_config: serde_json::json!({}),
                build_config: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        let source = active_node(db, &format!("source-{}", Uuid::now_v7().simple())).await;
        let target = active_node(db, &format!("target-{}", Uuid::now_v7().simple())).await;
        let deployment = create_deployment(db, &project, source.id, with_artifact, true).await;
        DeletionFixture {
            user,
            project,
            source,
            target,
            deployment,
        }
    }

    async fn enqueue_fixture(db: &DatabaseConnection, fixture: &DeletionFixture) {
        let transaction = db.begin().await.unwrap();
        scheduler::lock_placement(&transaction).await.unwrap();
        let source = node::Entity::find_by_id(fixture.source.id)
            .one(&transaction)
            .await
            .unwrap()
            .unwrap();
        enqueue(
            &transaction,
            source,
            Some(fixture.target.id),
            fixture.user.id,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }

    async fn reload_job(db: &DatabaseConnection, node_id: Uuid) -> node_deletion_job::Model {
        active_job(db, node_id).await.unwrap().unwrap()
    }

    async fn set_migration_status(
        db: &DatabaseConnection,
        job_id: Uuid,
        status: NodeDeploymentMigrationStatus,
        error: Option<&str>,
    ) {
        let migration = node_deployment_migration::Entity::find()
            .filter(node_deployment_migration::Column::JobId.eq(job_id))
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let ready = matches!(status, NodeDeploymentMigrationStatus::Ready);
        let mut active: node_deployment_migration::ActiveModel = migration.into();
        active.status = Set(status);
        active.error = Set(error.map(str::to_owned));
        active.ready_at = Set(ready.then(OffsetDateTime::now_utc));
        active.updated_at = Set(OffsetDateTime::now_utc());
        active.update(db).await.unwrap();
    }

    async fn audit_exists(
        db: &DatabaseConnection,
        node_id: Uuid,
        action: &str,
        result: AuditEventResult,
    ) -> bool {
        audit_event::Entity::find()
            .filter(audit_event::Column::TargetId.eq(node_id))
            .filter(audit_event::Column::Action.eq(action))
            .filter(audit_event::Column::Result.eq(result))
            .one(db)
            .await
            .unwrap()
            .is_some()
    }

    #[test]
    fn deletion_waits_for_shadow_migrations_then_active_builds() {
        assert_eq!(next_phase(2, 0, 0, 0), DeletionPhase::Migrating);
        assert_eq!(next_phase(2, 1, 0, 0), DeletionPhase::Migrating);
        assert_eq!(next_phase(2, 2, 0, 1), DeletionPhase::Draining);
        assert_eq!(next_phase(2, 2, 0, 0), DeletionPhase::Deleting);
        assert_eq!(next_phase(0, 0, 0, 2), DeletionPhase::Draining);
        assert_eq!(next_phase(0, 0, 0, 0), DeletionPhase::Deleting);
    }

    #[test]
    fn a_failed_shadow_copy_never_advances_to_route_switch_or_deletion() {
        assert_eq!(next_phase(3, 2, 1, 0), DeletionPhase::Failed);
    }

    #[tokio::test]
    async fn postgres_failed_migration_retries_then_cuts_over_and_drains_builds() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_fixture(&test_db.db, true).await;
        let active_build = create_deployment(
            &test_db.db,
            &fixture.project,
            fixture.source.id,
            true,
            false,
        )
        .await;
        let mut active: deployment::ActiveModel = active_build.into();
        active.build_status = Set(DeploymentBuildStatus::Building);
        active.build_node_id = Set(Some(fixture.source.id));
        active.serve_node_id = Set(None);
        active.serve_status = Set(DeploymentServeStatus::Retired);
        let active_build = active.update(&test_db.db).await.unwrap();

        enqueue_fixture(&test_db.db, &fixture).await;
        let job = reload_job(&test_db.db, fixture.source.id).await;
        set_migration_status(
            &test_db.db,
            job.id,
            NodeDeploymentMigrationStatus::Failed,
            Some("shadow copy failed"),
        )
        .await;
        process_pending_jobs(&test_db.db).await.unwrap();
        assert_eq!(
            reload_job(&test_db.db, fixture.source.id).await.status,
            NodeDeletionStatus::Failed
        );
        assert_eq!(
            deployment::Entity::find_by_id(fixture.deployment.id)
                .one(&test_db.db)
                .await
                .unwrap()
                .unwrap()
                .serve_node_id,
            Some(fixture.source.id)
        );
        assert!(
            audit_exists(
                &test_db.db,
                fixture.source.id,
                "node.deletion_failed",
                AuditEventResult::Failure,
            )
            .await
        );

        enqueue_fixture(&test_db.db, &fixture).await;
        assert!(
            audit_exists(
                &test_db.db,
                fixture.source.id,
                "node.deletion_retried",
                AuditEventResult::Success,
            )
            .await
        );
        let job = reload_job(&test_db.db, fixture.source.id).await;
        set_migration_status(
            &test_db.db,
            job.id,
            NodeDeploymentMigrationStatus::Ready,
            None,
        )
        .await;
        process_pending_jobs(&test_db.db).await.unwrap();
        let job = reload_job(&test_db.db, fixture.source.id).await;
        assert_eq!(job.status, NodeDeletionStatus::Draining);
        assert_eq!(job.active_builds, 1);
        assert_eq!(
            deployment::Entity::find_by_id(fixture.deployment.id)
                .one(&test_db.db)
                .await
                .unwrap()
                .unwrap()
                .serve_node_id,
            Some(fixture.target.id)
        );
        assert!(
            audit_exists(
                &test_db.db,
                fixture.source.id,
                "node.deletion_route_switched",
                AuditEventResult::Success,
            )
            .await
        );
        assert!(
            nodes::get_by_id(&test_db.db, fixture.source.id)
                .await
                .unwrap()
                .is_some()
        );

        let mut active: deployment::ActiveModel = active_build.into();
        active.build_status = Set(DeploymentBuildStatus::Ready);
        active.build_finished_at = Set(Some(OffsetDateTime::now_utc()));
        active.update(&test_db.db).await.unwrap();
        process_pending_jobs(&test_db.db).await.unwrap();
        assert_eq!(
            reload_job(&test_db.db, fixture.source.id).await.status,
            NodeDeletionStatus::Deleting
        );
        assert!(
            nodes::get_by_id(&test_db.db, fixture.source.id)
                .await
                .unwrap()
                .is_some()
        );
        process_pending_jobs(&test_db.db).await.unwrap();
        let completed = node_deletion_job::Entity::find_by_id(job.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(completed.status, NodeDeletionStatus::Completed);
        assert!(
            nodes::get_by_id(&test_db.db, fixture.source.id)
                .await
                .unwrap()
                .is_none()
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_missing_artifact_fails_without_cutover() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_fixture(&test_db.db, false).await;
        enqueue_fixture(&test_db.db, &fixture).await;

        process_pending_jobs(&test_db.db).await.unwrap();

        let job = reload_job(&test_db.db, fixture.source.id).await;
        assert_eq!(job.status, NodeDeletionStatus::Failed);
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("artifact"))
        );
        assert_eq!(
            deployment::Entity::find_by_id(fixture.deployment.id)
                .one(&test_db.db)
                .await
                .unwrap()
                .unwrap()
                .serve_node_id,
            Some(fixture.source.id)
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_shadow_ready_reports_do_not_change_the_live_deployment() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_fixture(&test_db.db, true).await;
        enqueue_fixture(&test_db.db, &fixture).await;
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        let report = ReportServeStatusRequest {
            status: ReportedServeStatus::Ready,
            failure_code: None,
            failure_message: None,
        };

        for _ in 0..2 {
            crate::features::api::v1::internal::serve::report_status(
                State(state.clone()),
                Extension(AuthenticatedNode(fixture.target.clone())),
                Path(fixture.deployment.id),
                Json(report.clone()),
            )
            .await
            .unwrap();
        }

        let migration = node_deployment_migration::Entity::find()
            .filter(node_deployment_migration::Column::DeploymentId.eq(fixture.deployment.id))
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(migration.status, NodeDeploymentMigrationStatus::Ready);
        let live = deployment::Entity::find_by_id(fixture.deployment.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(live.serve_node_id, Some(fixture.source.id));
        assert_eq!(live.serve_status, DeploymentServeStatus::Ready);
        assert_eq!(live.release_status, DeploymentReleaseStatus::Active);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_unhealthy_target_fails_before_route_cutover() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_fixture(&test_db.db, true).await;
        enqueue_fixture(&test_db.db, &fixture).await;
        let mut target: node::ActiveModel = fixture.target.clone().into();
        target.status = Set(NodeStatus::Offline);
        target.update(&test_db.db).await.unwrap();

        process_pending_jobs(&test_db.db).await.unwrap();

        let job = reload_job(&test_db.db, fixture.source.id).await;
        assert_eq!(job.status, NodeDeletionStatus::Failed);
        assert!(
            job.error
                .as_deref()
                .is_some_and(|error| error.contains("healthy"))
        );
        assert!(
            node_deployment_migration::Entity::find()
                .filter(node_deployment_migration::Column::JobId.eq(job.id))
                .all(&test_db.db)
                .await
                .unwrap()
                .into_iter()
                .all(|migration| matches!(migration.status, NodeDeploymentMigrationStatus::Failed))
        );
        assert_eq!(
            deployment::Entity::find_by_id(fixture.deployment.id)
                .one(&test_db.db)
                .await
                .unwrap()
                .unwrap()
                .serve_node_id,
            Some(fixture.source.id)
        );

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_stale_active_snapshot_cannot_claim_after_node_starts_draining() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        crate::infra::database::seed::run(&test_db.db)
            .await
            .unwrap();
        let fixture = seed_fixture(&test_db.db, true).await;
        let pending = deployments::create_deployment(
            &test_db.db,
            CreateDeploymentParams {
                project: fixture.project.clone(),
                environment: DeploymentEnvironment::Preview,
                triggered_by_user_id: None,
                branch: Some("claim-race".to_owned()),
                commit_hash: None,
                commit_message: None,
                preview_host: None,
                source_credential_version_id: None,
            },
            Placement {
                node_id: fixture.source.id,
                overcommitted: false,
                mode: PlacementMode::Automatic,
            },
        )
        .await
        .unwrap();
        let authenticated_snapshot = fixture.source.clone();
        let mut source: node::ActiveModel = fixture.source.clone().into();
        source.status = Set(NodeStatus::Draining);
        source.update(&test_db.db).await.unwrap();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(test_db.db.clone()).unwrap();
        assert!(
            state
                .cache
                .set(
                    CacheStore::connect_cache(CacheBackend::Moka, "")
                        .await
                        .unwrap(),
                )
                .is_ok()
        );

        let response = crate::features::api::v1::internal::deployments::claim(
            State(state),
            Extension(AuthenticatedNode(authenticated_snapshot)),
            Json(ClaimRequest { capacity: 1 }),
        )
        .await
        .unwrap()
        .into_response();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(body["data"]["deployment"].is_null());
        let pending = deployment::Entity::find_by_id(pending.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pending.build_status, DeploymentBuildStatus::Pending);
        assert_eq!(pending.build_node_id, None);

        test_db.cleanup().await;
    }

    #[tokio::test]
    async fn postgres_plan_requires_whole_batch_capacity_and_zero_work_has_deleting_phase() {
        let Some(test_db) = PostgresTestDatabase::start().await else {
            eprintln!("GRASS_TEST_DATABASE_URL is not configured; skipping PostgreSQL test");
            return;
        };
        let fixture = seed_fixture(&test_db.db, true).await;
        create_deployment(
            &test_db.db,
            &fixture.project,
            fixture.source.id,
            true,
            false,
        )
        .await;
        let mut target: node::ActiveModel = fixture.target.clone().into();
        target.capacity_cpu_millicores = Set(50);
        target.capacity_memory_mb = Set(64);
        target.capacity_disk_mb = Set(256);
        target.max_deployments = Set(1);
        target.update(&test_db.db).await.unwrap();
        let plan = plan(&test_db.db, &fixture.source).await.unwrap();
        assert!(plan.requires_target);
        assert_eq!(plan.assigned_deployments, 2);
        assert!(plan.eligible_targets.is_empty());

        let mut first: deployment::ActiveModel = fixture.deployment.clone().into();
        first.build_status = Set(DeploymentBuildStatus::Failed);
        first.update(&test_db.db).await.unwrap();
        deployment::Entity::update_many()
            .col_expr(
                deployment::Column::BuildStatus,
                sea_orm::ActiveEnum::as_enum(&DeploymentBuildStatus::Failed),
            )
            .filter(deployment::Column::ServeNodeId.eq(fixture.source.id))
            .exec(&test_db.db)
            .await
            .unwrap();
        let source = node::Entity::find_by_id(fixture.source.id)
            .one(&test_db.db)
            .await
            .unwrap()
            .unwrap();
        let transaction = test_db.db.begin().await.unwrap();
        scheduler::lock_placement(&transaction).await.unwrap();
        enqueue(&transaction, source, None, fixture.user.id)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        process_pending_jobs(&test_db.db).await.unwrap();
        assert_eq!(
            reload_job(&test_db.db, fixture.source.id).await.status,
            NodeDeletionStatus::Deleting
        );
        assert!(
            nodes::get_by_id(&test_db.db, fixture.source.id)
                .await
                .unwrap()
                .is_some()
        );
        process_pending_jobs(&test_db.db).await.unwrap();
        assert!(
            nodes::get_by_id(&test_db.db, fixture.source.id)
                .await
                .unwrap()
                .is_none()
        );

        test_db.cleanup().await;
    }
}
