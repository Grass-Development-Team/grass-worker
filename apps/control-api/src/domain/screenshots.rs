use std::{collections::HashMap, io::Cursor};

use image::{ImageFormat, ImageReader, Limits};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, TransactionTrait,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    features::api::v1::preview_auth,
    infra::{
        database::{
            entity::{
                DeploymentArtifactKind, DeploymentBuildStatus, DeploymentEnvironment,
                DeploymentReleaseStatus, DeploymentScreenshotStatus, DeploymentServeStatus,
                deployment, deployment_artifact, deployment_screenshot_job,
            },
            is_unique_violation,
        },
        screenshot::{CaptureRequest, from_config},
    },
    state::ControlApiState,
};

const MAX_ATTEMPTS: i32 = 4;
const STALE_RUNNING_SECONDS: i64 = 120;

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct SweepResult {
    pub enqueued: u64,
    pub succeeded: u64,
    pub retried: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScreenshotState {
    Pending,
    Ready(Uuid),
    Unavailable,
}

pub fn artifact_key(project_id: Uuid, deployment_id: Uuid, artifact_id: Uuid) -> String {
    format!("deployments/{project_id}/{deployment_id}/screenshots/{artifact_id}.webp")
}

fn retry_delay(attempt_count: i32) -> Duration {
    match attempt_count {
        0 | 1 => Duration::seconds(10),
        2 => Duration::seconds(30),
        _ => Duration::minutes(2),
    }
}

fn bounded_error(error: &anyhow::Error) -> String {
    error.to_string().chars().take(500).collect::<String>()
}

fn failure_outcome(attempt_count: i32) -> (DeploymentScreenshotStatus, bool) {
    if attempt_count < MAX_ATTEMPTS {
        (DeploymentScreenshotStatus::Pending, true)
    } else {
        (DeploymentScreenshotStatus::Failed, false)
    }
}

pub fn eligible(deployment: &deployment::Model) -> bool {
    matches!(deployment.environment, DeploymentEnvironment::Production)
        && matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        && matches!(deployment.serve_status, DeploymentServeStatus::Ready)
        && matches!(deployment.release_status, DeploymentReleaseStatus::Active)
        && deployment.preview_host.is_some()
        && deployment.deleted_at.is_none()
}

pub async fn sweep(state: &ControlApiState) -> anyhow::Result<SweepResult> {
    let config = state.config.read().unwrap().screenshot.clone();
    let Some(config) = config else {
        return Ok(SweepResult::default());
    };
    let Some(db) = state.try_database() else {
        return Ok(SweepResult::default());
    };
    let mut result = SweepResult {
        enqueued: enqueue_current(db).await?,
        ..SweepResult::default()
    };
    let (stale_retried, stale_failed) = reset_stale_jobs(db).await?;
    result.retried += stale_retried;
    result.failed += stale_failed;
    let Some(job) = claim_next(db).await? else {
        return Ok(result);
    };
    match capture_job(state, &config, &job).await {
        Ok(()) => result.succeeded += 1,
        Err(error) => {
            let retry = record_failure(db, &job, &error).await?;
            if retry {
                result.retried += 1;
            } else {
                result.failed += 1;
            }
        }
    }
    Ok(result)
}

async fn enqueue_current(db: &sea_orm::DatabaseConnection) -> anyhow::Result<u64> {
    let candidates = deployment::Entity::find()
        .filter(deployment::Column::Environment.eq(DeploymentEnvironment::Production))
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
        .filter(deployment::Column::ServeStatus.eq(DeploymentServeStatus::Ready))
        .filter(deployment::Column::ReleaseStatus.eq(DeploymentReleaseStatus::Active))
        .filter(deployment::Column::PreviewHost.is_not_null())
        .filter(deployment::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let existing = deployment_screenshot_job::Entity::find()
        .filter(
            deployment_screenshot_job::Column::DeploymentId
                .is_in(candidates.iter().map(|item| item.id)),
        )
        .all(db)
        .await?
        .into_iter()
        .map(|job| job.deployment_id)
        .collect::<std::collections::HashSet<_>>();
    let now = OffsetDateTime::now_utc();
    let mut enqueued = 0;
    for candidate in candidates {
        if existing.contains(&candidate.id) {
            continue;
        }
        let insert = deployment_screenshot_job::ActiveModel {
            deployment_id: Set(candidate.id),
            status: Set(DeploymentScreenshotStatus::Pending),
            attempt_count: Set(0),
            next_attempt_at: Set(now),
            last_error: Set(None),
            artifact_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await;
        match insert {
            Ok(_) => enqueued += 1,
            Err(error) if is_unique_violation(&error.clone().into()) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(enqueued)
}

async fn reset_stale_jobs(db: &sea_orm::DatabaseConnection) -> anyhow::Result<(u64, u64)> {
    let now = OffsetDateTime::now_utc();
    let stale_before = now - Duration::seconds(STALE_RUNNING_SECONDS);
    let retried = deployment_screenshot_job::Entity::update_many()
        .col_expr(
            deployment_screenshot_job::Column::Status,
            sea_orm::sea_query::Expr::value(DeploymentScreenshotStatus::Pending),
        )
        .col_expr(
            deployment_screenshot_job::Column::NextAttemptAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .col_expr(
            deployment_screenshot_job::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(deployment_screenshot_job::Column::Status.eq(DeploymentScreenshotStatus::Running))
        .filter(deployment_screenshot_job::Column::AttemptCount.lt(MAX_ATTEMPTS))
        .filter(deployment_screenshot_job::Column::UpdatedAt.lte(stale_before))
        .exec(db)
        .await?;
    let failed = deployment_screenshot_job::Entity::update_many()
        .col_expr(
            deployment_screenshot_job::Column::Status,
            sea_orm::sea_query::Expr::value(DeploymentScreenshotStatus::Failed),
        )
        .col_expr(
            deployment_screenshot_job::Column::LastError,
            sea_orm::sea_query::Expr::value(Some(
                "screenshot capture was interrupted during the final attempt".to_owned(),
            )),
        )
        .col_expr(
            deployment_screenshot_job::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(deployment_screenshot_job::Column::Status.eq(DeploymentScreenshotStatus::Running))
        .filter(deployment_screenshot_job::Column::AttemptCount.gte(MAX_ATTEMPTS))
        .filter(deployment_screenshot_job::Column::UpdatedAt.lte(stale_before))
        .exec(db)
        .await?;
    Ok((retried.rows_affected, failed.rows_affected))
}

async fn claim_next(
    db: &sea_orm::DatabaseConnection,
) -> anyhow::Result<Option<deployment_screenshot_job::Model>> {
    let now = OffsetDateTime::now_utc();
    let transaction = db.begin().await?;
    let Some(job) = deployment_screenshot_job::Entity::find()
        .filter(deployment_screenshot_job::Column::Status.eq(DeploymentScreenshotStatus::Pending))
        .filter(deployment_screenshot_job::Column::AttemptCount.lt(MAX_ATTEMPTS))
        .filter(deployment_screenshot_job::Column::NextAttemptAt.lte(now))
        .order_by_asc(deployment_screenshot_job::Column::NextAttemptAt)
        .lock_exclusive()
        .one(&transaction)
        .await?
    else {
        transaction.commit().await?;
        return Ok(None);
    };
    let mut active: deployment_screenshot_job::ActiveModel = job.into();
    active.status = Set(DeploymentScreenshotStatus::Running);
    active.attempt_count = Set(active.attempt_count.as_ref() + 1);
    active.updated_at = Set(now);
    let job = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(Some(job))
}

async fn capture_job(
    state: &ControlApiState,
    config: &crate::infra::config::screenshot::ScreenshotConfig,
    job: &deployment_screenshot_job::Model,
) -> anyhow::Result<()> {
    let db = state
        .try_database()
        .ok_or_else(|| anyhow::anyhow!("database not available"))?;
    let deployment = deployment::Entity::find_by_id(job.deployment_id)
        .one(db)
        .await?
        .filter(eligible)
        .ok_or_else(|| anyhow::anyhow!("deployment is no longer eligible for a screenshot"))?;
    let host = deployment
        .preview_host
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("deployment has no preview host"))?;
    let grant = preview_auth::issue_screenshot_grant(state, host).await?;
    let png = from_config(config)
        .capture(CaptureRequest {
            url: grant.target_url,
            cookie_name: grant.cookie_name,
            cookie_value: grant.token,
        })
        .await?;
    let webp = encode_webp(&png)?;
    let artifact_id = Uuid::now_v7();
    let key = artifact_key(deployment.project_id, deployment.id, artifact_id);
    let storage = state.storage.clone();
    let stored = storage.write_bytes(&key, &webp).await?;
    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            let _ = storage.remove(&key).await;
            return Err(error.into());
        }
    };
    let now = OffsetDateTime::now_utc();
    let insert = deployment_artifact::ActiveModel {
        id: Set(artifact_id),
        deployment_id: Set(deployment.id),
        kind: Set(DeploymentArtifactKind::Screenshot),
        storage_path: Set(stored.relative_path),
        checksum_sha256: Set(Some(stored.checksum_sha256)),
        size_bytes: Set(Some(stored.size_bytes)),
        manifest: Set(serde_json::json!({
            "provider": "chromium",
            "width": 1280,
            "height": 720,
            "format": "webp",
        })),
        deleted_at: Set(None),
        created_at: Set(now),
    }
    .insert(&transaction)
    .await;
    if let Err(error) = insert {
        let _ = transaction.rollback().await;
        let _ = storage.remove(&key).await;
        return Err(error.into());
    }
    let mut active: deployment_screenshot_job::ActiveModel = job.clone().into();
    active.status = Set(DeploymentScreenshotStatus::Succeeded);
    active.artifact_id = Set(Some(artifact_id));
    active.last_error = Set(None);
    active.updated_at = Set(now);
    if let Err(error) = active.update(&transaction).await {
        let _ = transaction.rollback().await;
        let _ = storage.remove(&key).await;
        return Err(error.into());
    }
    if let Err(error) = transaction.commit().await {
        let _ = storage.remove(&key).await;
        return Err(error.into());
    }
    Ok(())
}

async fn record_failure(
    db: &sea_orm::DatabaseConnection,
    job: &deployment_screenshot_job::Model,
    error: &anyhow::Error,
) -> anyhow::Result<bool> {
    let now = OffsetDateTime::now_utc();
    let (status, retry) = failure_outcome(job.attempt_count);
    let mut active: deployment_screenshot_job::ActiveModel = job.clone().into();
    active.status = Set(status);
    active.next_attempt_at = Set(now + retry_delay(job.attempt_count));
    active.last_error = Set(Some(bounded_error(error)));
    active.artifact_id = Set(None);
    active.updated_at = Set(now);
    active.update(db).await?;
    Ok(retry)
}

fn encode_webp(png: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut reader = ImageReader::with_format(Cursor::new(png), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(1280);
    limits.max_image_height = Some(720);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?;
    if image.width() != 1280 || image.height() != 720 {
        anyhow::bail!("Chromium screenshot must be 1280 by 720 pixels");
    }
    let rgba = image.into_rgba8();
    Ok(webp::Encoder::from_rgba(&rgba, 1280, 720)
        .encode(80.0)
        .to_vec())
}

pub async fn states_for(
    db: &sea_orm::DatabaseConnection,
    deployment_ids: &[Uuid],
) -> anyhow::Result<HashMap<Uuid, ScreenshotState>> {
    if deployment_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let jobs = deployment_screenshot_job::Entity::find()
        .filter(
            deployment_screenshot_job::Column::DeploymentId.is_in(deployment_ids.iter().copied()),
        )
        .all(db)
        .await?;
    let artifact_ids = jobs
        .iter()
        .filter_map(|job| job.artifact_id)
        .collect::<Vec<_>>();
    let available = if artifact_ids.is_empty() {
        std::collections::HashSet::new()
    } else {
        deployment_artifact::Entity::find()
            .filter(deployment_artifact::Column::Id.is_in(artifact_ids))
            .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::Screenshot))
            .filter(deployment_artifact::Column::DeletedAt.is_null())
            .all(db)
            .await?
            .into_iter()
            .map(|artifact| artifact.id)
            .collect()
    };
    Ok(jobs
        .into_iter()
        .map(|job| {
            let state = match job.status {
                DeploymentScreenshotStatus::Pending | DeploymentScreenshotStatus::Running => {
                    ScreenshotState::Pending
                }
                DeploymentScreenshotStatus::Succeeded => job
                    .artifact_id
                    .filter(|id| available.contains(id))
                    .map(ScreenshotState::Ready)
                    .unwrap_or(ScreenshotState::Unavailable),
                DeploymentScreenshotStatus::Failed => ScreenshotState::Unavailable,
            };
            (job.deployment_id, state)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    use sea_orm::{DbBackend, MockDatabase, MockExecResult};

    use super::*;

    #[test]
    fn screenshot_keys_follow_the_deployment_object_prefix() {
        let project = Uuid::nil();
        let deployment = Uuid::max();
        let artifact = Uuid::parse_str("018f47e2-3d62-7cc3-b0fd-8a73f01b2a11").unwrap();
        assert_eq!(
            artifact_key(project, deployment, artifact),
            format!("deployments/{project}/{deployment}/screenshots/{artifact}.webp")
        );
    }

    #[test]
    fn screenshot_retries_are_bounded_and_back_off() {
        assert_eq!(retry_delay(1), Duration::seconds(10));
        assert_eq!(retry_delay(2), Duration::seconds(30));
        assert_eq!(retry_delay(3), Duration::minutes(2));
        assert_eq!(
            failure_outcome(2),
            (DeploymentScreenshotStatus::Pending, true)
        );
        assert_eq!(
            failure_outcome(3),
            (DeploymentScreenshotStatus::Pending, true)
        );
        assert_eq!(
            failure_outcome(4),
            (DeploymentScreenshotStatus::Failed, false)
        );
    }

    #[test]
    fn screenshot_png_is_reencoded_as_webp() {
        let pixels = vec![20_u8; 1280 * 720 * 4];
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&pixels, 1280, 720, ExtendedColorType::Rgba8)
            .unwrap();
        let webp = encode_webp(&png).unwrap();
        assert!(webp.starts_with(b"RIFF"));
        assert_eq!(&webp[8..12], b"WEBP");
    }

    #[tokio::test]
    async fn stale_jobs_retry_unfinished_attempts_and_fail_an_exhausted_attempt() {
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_exec_results([
                MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 2,
                },
                MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
            ])
            .into_connection();

        assert_eq!(reset_stale_jobs(&db).await.unwrap(), (2, 1));
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("attempt_count"));
        assert!(statements.contains("screenshot capture was interrupted during the final attempt"));
    }

    #[tokio::test]
    async fn screenshot_states_distinguish_pending_ready_failed_and_missing_artifacts() {
        let now = OffsetDateTime::now_utc();
        let pending_id = Uuid::now_v7();
        let ready_id = Uuid::now_v7();
        let failed_id = Uuid::now_v7();
        let missing_id = Uuid::now_v7();
        let artifact_id = Uuid::now_v7();
        let missing_artifact_id = Uuid::now_v7();
        let job = |deployment_id, status, artifact_id| deployment_screenshot_job::Model {
            deployment_id,
            status,
            attempt_count: 1,
            next_attempt_at: now,
            last_error: None,
            artifact_id,
            created_at: now,
            updated_at: now,
        };
        let jobs = vec![
            job(pending_id, DeploymentScreenshotStatus::Running, None),
            job(
                ready_id,
                DeploymentScreenshotStatus::Succeeded,
                Some(artifact_id),
            ),
            job(failed_id, DeploymentScreenshotStatus::Failed, None),
            job(
                missing_id,
                DeploymentScreenshotStatus::Succeeded,
                Some(missing_artifact_id),
            ),
        ];
        let artifact = deployment_artifact::Model {
            id: artifact_id,
            deployment_id: ready_id,
            kind: DeploymentArtifactKind::Screenshot,
            storage_path: artifact_key(Uuid::now_v7(), ready_id, artifact_id),
            checksum_sha256: Some("a".repeat(64)),
            size_bytes: Some(1024),
            manifest: serde_json::json!({ "format": "webp" }),
            deleted_at: None,
            created_at: now,
        };
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([jobs])
            .append_query_results([[artifact]])
            .into_connection();

        let states = states_for(&db, &[pending_id, ready_id, failed_id, missing_id])
            .await
            .unwrap();

        assert_eq!(states[&pending_id], ScreenshotState::Pending);
        assert_eq!(states[&ready_id], ScreenshotState::Ready(artifact_id));
        assert_eq!(states[&failed_id], ScreenshotState::Unavailable);
        assert_eq!(states[&missing_id], ScreenshotState::Unavailable);
    }
}
