//! Deployment artifact retention and tombstone cleanup.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    domain::{quotas::QuotaDimension, settings},
    infra::{
        database::entity::{
            DeploymentArtifactKind, DeploymentBuildStatus, DeploymentEnvironment,
            DeploymentReleaseStatus, DeploymentServeStatus, deployment, deployment_artifact,
            node_deployment_migration,
        },
        quota::QuotaService,
        storage::LocalStorage,
    },
};

pub const LOG_RETENTION_DAYS_KEY: &str = "artifact_retention.log_days";
pub const PREVIEW_RETENTION_DAYS_KEY: &str = "artifact_retention.preview_days";
pub const FAILED_RETENTION_DAYS_KEY: &str = "artifact_retention.failed_days";
pub const PRODUCTION_KEEP_KEY: &str = "artifact_retention.production_keep";

pub const DEFAULT_LOG_RETENTION_DAYS: u64 = 90;
pub const DEFAULT_PREVIEW_RETENTION_DAYS: u64 = 7;
pub const DEFAULT_FAILED_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_PRODUCTION_KEEP: u64 = 10;
pub const BYTES_PER_MB: i64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub log_retention_days: u64,
    pub preview_retention_days: u64,
    pub failed_retention_days: u64,
    pub production_keep: u64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            log_retention_days: DEFAULT_LOG_RETENTION_DAYS,
            preview_retention_days: DEFAULT_PREVIEW_RETENTION_DAYS,
            failed_retention_days: DEFAULT_FAILED_RETENTION_DAYS,
            production_keep: DEFAULT_PRODUCTION_KEEP,
        }
    }
}

impl RetentionPolicy {
    pub async fn load<C: ConnectionTrait>(db: &C) -> anyhow::Result<Self> {
        Ok(Self {
            log_retention_days: setting_u64(db, LOG_RETENTION_DAYS_KEY, DEFAULT_LOG_RETENTION_DAYS)
                .await?,
            preview_retention_days: setting_u64(
                db,
                PREVIEW_RETENTION_DAYS_KEY,
                DEFAULT_PREVIEW_RETENTION_DAYS,
            )
            .await?,
            failed_retention_days: setting_u64(
                db,
                FAILED_RETENTION_DAYS_KEY,
                DEFAULT_FAILED_RETENTION_DAYS,
            )
            .await?,
            production_keep: setting_u64(db, PRODUCTION_KEEP_KEY, DEFAULT_PRODUCTION_KEEP).await?,
        })
    }
}

async fn setting_u64<C: ConnectionTrait>(db: &C, key: &str, default: u64) -> anyhow::Result<u64> {
    let Some(setting) = settings::get_setting(db, key).await? else {
        return Ok(default);
    };
    Ok(setting
        .value
        .as_u64()
        .or_else(|| setting.value.as_str().and_then(|value| value.parse().ok()))
        .unwrap_or(default))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepResult {
    pub tombstoned: u64,
    pub removed: u64,
    pub failed: u64,
}

/// Returns whether a deployment may be retained forever regardless of the
/// configured age/count policy.
pub fn deployment_is_protected(item: &deployment::Model, migrating: bool) -> bool {
    let active_or_pending_release = matches!(
        item.release_status,
        DeploymentReleaseStatus::Active
            | DeploymentReleaseStatus::PendingReview
            | DeploymentReleaseStatus::Approved
    ) || item.pending_release_requested_at.is_some();
    let in_progress = matches!(
        item.build_status,
        DeploymentBuildStatus::Pending
            | DeploymentBuildStatus::Claimed
            | DeploymentBuildStatus::Queued
            | DeploymentBuildStatus::Building
    );
    let assigned = item.serve_node_id.is_some()
        && !matches!(item.serve_status, DeploymentServeStatus::Retired);
    active_or_pending_release || in_progress || assigned || migrating
}

pub fn artifact_expired(
    artifact: &deployment_artifact::Model,
    deployment: &deployment::Model,
    production_rank: Option<u64>,
    protected: bool,
    policy: RetentionPolicy,
    now: OffsetDateTime,
) -> bool {
    if artifact.deleted_at.is_some() {
        return true;
    }
    if protected {
        return false;
    }
    if matches!(artifact.kind, DeploymentArtifactKind::BuildLog) {
        return older_than(artifact.created_at, policy.log_retention_days, now);
    }
    match deployment.environment {
        DeploymentEnvironment::Production => {
            policy.production_keep > 0
                && production_rank.is_some_and(|rank| rank >= policy.production_keep)
        }
        DeploymentEnvironment::Preview => {
            if matches!(
                deployment.build_status,
                DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
            ) {
                older_than(deployment.created_at, policy.failed_retention_days, now)
            } else {
                older_than(deployment.created_at, policy.preview_retention_days, now)
            }
        }
    }
}

fn older_than(created_at: OffsetDateTime, days: u64, now: OffsetDateTime) -> bool {
    days > 0 && created_at <= now - Duration::days(days as i64)
}

/// Performs one retention sweep. Tombstones are written before filesystem
/// deletion so an interrupted or failed delete is retried on the next pass.
pub async fn sweep(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    storage: &LocalStorage,
    policy: RetentionPolicy,
    now: OffsetDateTime,
) -> anyhow::Result<SweepResult> {
    let deployments = deployment::Entity::find()
        .filter(deployment::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    if deployments.is_empty() {
        return Ok(SweepResult::default());
    }
    let deployment_ids = deployments.iter().map(|item| item.id).collect::<Vec<_>>();
    let migrating = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::DeploymentId.is_in(deployment_ids.clone()))
        .all(db)
        .await?
        .into_iter()
        .map(|item| item.deployment_id)
        .collect::<HashSet<_>>();
    let deployment_map = deployments
        .iter()
        .map(|item| (item.id, item))
        .collect::<HashMap<_, _>>();
    let mut production_ranks = HashMap::new();
    let mut production_by_project = HashMap::<Uuid, Vec<&deployment::Model>>::new();
    for item in &deployments {
        if matches!(item.environment, DeploymentEnvironment::Production) {
            production_by_project
                .entry(item.project_id)
                .or_default()
                .push(item);
        }
    }
    for items in production_by_project.values_mut() {
        items.sort_by_key(|item| Reverse(item.created_at));
        for (rank, item) in items.iter().enumerate() {
            production_ranks.insert(item.id, rank as u64);
        }
    }

    let artifacts = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.is_in(deployment_ids))
        .all(db)
        .await?;
    let quota = QuotaService::new(db, cache);
    let mut result = SweepResult::default();
    for artifact in artifacts {
        let Some(deployment) = deployment_map.get(&artifact.deployment_id) else {
            continue;
        };
        let protected = deployment_is_protected(deployment, migrating.contains(&deployment.id));
        if !artifact_expired(
            &artifact,
            deployment,
            production_ranks.get(&deployment.id).copied(),
            protected,
            policy,
            now,
        ) {
            continue;
        }

        let mut marked = artifact.clone();
        if marked.deleted_at.is_none() {
            marked.deleted_at = Some(now);
            let active: deployment_artifact::ActiveModel = marked.clone().into();
            active.update(db).await?;
            result.tombstoned += 1;
        }

        if matches!(artifact.kind, DeploymentArtifactKind::GrassOutput)
            && let Some(size_bytes) = artifact.size_bytes
        {
            let size_mb = (size_bytes.max(0) + BYTES_PER_MB - 1) / BYTES_PER_MB;
            if size_mb > 0 {
                if let Err(error) = quota
                    .release_once(
                        "retention.artifact",
                        deployment.team_id,
                        &[crate::infra::quota::QuotaCharge::amount(
                            QuotaDimension::StorageMb,
                            size_mb,
                        )],
                        "deployment_artifact",
                        artifact.id,
                    )
                    .await
                {
                    tracing::warn!(operation = "artifact_retention.release_quota", %error, artifact_id = %artifact.id, "failed to release artifact quota");
                }
            }
        }

        match storage.remove(&artifact.storage_path).await {
            Ok(()) => result.removed += 1,
            Err(error) => {
                result.failed += 1;
                tracing::warn!(operation = "artifact_retention.remove_file", %error, artifact_id = %artifact.id, "failed to remove artifact file; tombstone will be retried");
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::{AuditEventVisibility, ProjectRuntime, ReleaseReason};

    fn deployment(environment: DeploymentEnvironment) -> deployment::Model {
        let now = OffsetDateTime::now_utc();
        deployment::Model {
            id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            build_node_id: None,
            serve_node_id: None,
            environment,
            runtime_kind: ProjectRuntime::Static,
            build_status: DeploymentBuildStatus::Failed,
            serve_status: DeploymentServeStatus::Retired,
            release_status: DeploymentReleaseStatus::Draft,
            serve_cpu_millicores: 0,
            serve_memory_mb: 0,
            serve_disk_mb: 0,
            overcommitted: false,
            source_repository_url: None,
            source_credential_version_id: None,
            source_branch: None,
            commit_hash: None,
            commit_message: None,
            triggered_by_user_id: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_metadata: serde_json::json!({}),
            preview_host: None,
            build_stage: None,
            failure_code: None,
            failure_message: None,
            serve_failure_code: None,
            serve_failure_message: None,
            pending_release_reason: None::<ReleaseReason>,
            pending_release_actor_user_id: None,
            pending_release_audit_visibility: None::<AuditEventVisibility>,
            pending_release_requested_at: None,
            claimed_at: None,
            build_started_at: None,
            build_finished_at: None,
            serve_started_at: None,
            serve_finished_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn artifact(
        kind: DeploymentArtifactKind,
        created_at: OffsetDateTime,
    ) -> deployment_artifact::Model {
        deployment_artifact::Model {
            id: Uuid::now_v7(),
            deployment_id: Uuid::now_v7(),
            kind,
            storage_path: "deployments/test/file".to_owned(),
            checksum_sha256: None,
            size_bytes: Some(1),
            manifest: serde_json::json!({}),
            deleted_at: None,
            created_at,
        }
    }

    #[test]
    fn zero_disables_each_retention_rule() {
        let now = OffsetDateTime::now_utc();
        let item = deployment(DeploymentEnvironment::Preview);
        let log = artifact(DeploymentArtifactKind::BuildLog, now - Duration::days(365));
        let policy = RetentionPolicy {
            log_retention_days: 0,
            preview_retention_days: 0,
            failed_retention_days: 0,
            production_keep: 0,
        };
        assert!(!artifact_expired(&log, &item, None, false, policy, now));
        assert!(!artifact_expired(
            &artifact(
                DeploymentArtifactKind::GrassOutput,
                now - Duration::days(365)
            ),
            &item,
            None,
            false,
            policy,
            now
        ));
    }

    #[test]
    fn protected_deployments_are_never_expired() {
        let now = OffsetDateTime::now_utc();
        let mut item = deployment(DeploymentEnvironment::Preview);
        item.release_status = DeploymentReleaseStatus::Active;
        let policy = RetentionPolicy::default();
        assert!(!artifact_expired(
            &artifact(
                DeploymentArtifactKind::GrassOutput,
                now - Duration::days(365)
            ),
            &item,
            None,
            true,
            policy,
            now
        ));
    }

    #[test]
    fn production_rank_keeps_the_newest_entries() {
        let now = OffsetDateTime::now_utc();
        let item = deployment(DeploymentEnvironment::Production);
        let policy = RetentionPolicy {
            production_keep: 2,
            ..RetentionPolicy::default()
        };
        let old = artifact(
            DeploymentArtifactKind::GrassOutput,
            now - Duration::days(365),
        );
        assert!(artifact_expired(&old, &item, Some(2), false, policy, now));
        assert!(!artifact_expired(&old, &item, Some(1), false, policy, now));
    }

    #[test]
    fn screenshots_follow_their_deployments_retention_rank() {
        let now = OffsetDateTime::now_utc();
        let item = deployment(DeploymentEnvironment::Production);
        let screenshot = artifact(
            DeploymentArtifactKind::Screenshot,
            now - Duration::days(365),
        );
        let policy = RetentionPolicy {
            production_keep: 2,
            ..RetentionPolicy::default()
        };

        assert!(artifact_expired(
            &screenshot,
            &item,
            Some(2),
            false,
            policy,
            now,
        ));
        assert!(!artifact_expired(
            &screenshot,
            &item,
            Some(0),
            false,
            policy,
            now,
        ));
    }
}
