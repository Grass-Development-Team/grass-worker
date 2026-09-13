use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{deployments, nodes, scheduler},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{
            AuditEventResult, DeploymentArtifactKind, DeploymentBuildStatus, DeploymentServeStatus,
            NodeDeletionStatus, NodeDeploymentMigrationStatus, NodeStatus, deployment,
            deployment_artifact, node, node_deletion_job, node_deployment_migration,
        },
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
    transaction: &crate::infra::audit::AuditTransaction,
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
    transaction: &crate::infra::audit::AuditTransaction,
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
    transaction: &crate::infra::audit::AuditTransaction,
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
    transaction: &crate::infra::audit::AuditTransaction,
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
    transaction: &crate::infra::audit::AuditTransaction,
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
    let transaction = crate::infra::audit::AuditTransaction::begin(db).await?;
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
            let transaction = crate::infra::audit::AuditTransaction::begin(db).await?;
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
mod tests;
