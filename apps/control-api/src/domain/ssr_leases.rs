//! Control-plane truth for SSR process and hourly runtime leases.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QuerySelect, TransactionTrait,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    domain::quotas::{self, QuotaDimension, RecordEventParams},
    infra::database::entity::{
        DeploymentBuildStatus, DeploymentReleaseStatus, ProjectRuntime, QuotaEventKind, deployment,
        ssr_process_lease, team,
    },
};

pub const LEASE_TTL: Duration = Duration::seconds(90);
pub const HOUR_BLOCK: Duration = Duration::hours(1);

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("deployment is not an SSR Serve assignment for this node")]
    NotAssigned,
    #[error("SSR process quota exceeded")]
    ProcessQuota,
    #[error("SSR monthly hour quota exceeded")]
    HourQuota,
    #[error("SSR lease not found")]
    NotFound,
    #[error("SSR lease belongs to another node")]
    WrongNode,
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

pub async fn acquire(
    db: &sea_orm::DatabaseConnection,
    deployment_id: Uuid,
    node_id: Uuid,
    now: OffsetDateTime,
) -> Result<ssr_process_lease::Model, LeaseError> {
    let transaction = db.begin().await.map_err(db_error)?;
    let deployment = deployment::Entity::find_by_id(deployment_id)
        .filter(deployment::Column::DeletedAt.is_null())
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(db_error)?
        .ok_or(LeaseError::NotAssigned)?;
    if deployment.runtime_kind != ProjectRuntime::Ssr
        || deployment.serve_node_id != Some(node_id)
        || !matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        || matches!(deployment.release_status, DeploymentReleaseStatus::Rejected)
    {
        return Err(LeaseError::NotAssigned);
    }

    let expired_lease = ssr_process_lease::Entity::find()
        .filter(ssr_process_lease::Column::DeploymentId.eq(deployment_id))
        .filter(ssr_process_lease::Column::NodeId.eq(node_id))
        .filter(ssr_process_lease::Column::ReleasedAt.is_null())
        .filter(ssr_process_lease::Column::ExpiresAt.lte(now))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(db_error)?;
    if let Some(expired) = &expired_lease {
        let mut active: ssr_process_lease::ActiveModel = expired.clone().into();
        active.released_at = Set(Some(now));
        active.update(&transaction).await.map_err(db_error)?;
    }

    if let Some(existing) = ssr_process_lease::Entity::find()
        .filter(ssr_process_lease::Column::DeploymentId.eq(deployment_id))
        .filter(ssr_process_lease::Column::NodeId.eq(node_id))
        .filter(ssr_process_lease::Column::ReleasedAt.is_null())
        .one(&transaction)
        .await
        .map_err(db_error)?
    {
        transaction.commit().await.map_err(db_error)?;
        return Ok(existing);
    }

    let team = team::Entity::find_by_id(deployment.team_id)
        .filter(team::Column::DeletedAt.is_null())
        .one(&transaction)
        .await
        .map_err(db_error)?
        .ok_or(LeaseError::NotAssigned)?;
    let resolved = quotas::resolve_team_quota(&transaction, &team)
        .await
        .map_err(db_error)?;
    let process_count = ssr_process_lease::Entity::find()
        .filter(ssr_process_lease::Column::TeamId.eq(team.id))
        .filter(ssr_process_lease::Column::ReleasedAt.is_null())
        .filter(ssr_process_lease::Column::ExpiresAt.gt(now))
        .count(&transaction)
        .await
        .map_err(db_error)? as i64;
    if resolved
        .limit_for(QuotaDimension::SsrProcesses)
        .is_some_and(|limit| process_count >= limit)
    {
        return Err(LeaseError::ProcessQuota);
    }
    let hour_usage =
        quotas::effective_usage(&transaction, team.id, QuotaDimension::SsrHoursMonthly)
            .await
            .map_err(db_error)?;
    if resolved
        .limit_for(QuotaDimension::SsrHoursMonthly)
        .is_some_and(|limit| hour_usage >= limit)
    {
        return Err(LeaseError::HourQuota);
    }

    let lease = ssr_process_lease::ActiveModel {
        id: Set(Uuid::now_v7()),
        deployment_id: Set(deployment_id),
        team_id: Set(team.id),
        node_id: Set(node_id),
        started_at: Set(now),
        renewed_at: Set(now),
        expires_at: Set(now + LEASE_TTL),
        hour_block_start: Set(now),
        released_at: Set(None),
    }
    .insert(&transaction)
    .await
    .map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    if let Some(expired) = expired_lease {
        release_process_event(db, &expired).await?;
    }
    record_process_event(db, &lease).await?;
    record_block(db, &lease, now).await?;
    Ok(lease)
}

pub async fn renew(
    db: &sea_orm::DatabaseConnection,
    lease_id: Uuid,
    deployment_id: Uuid,
    node_id: Uuid,
    now: OffsetDateTime,
) -> Result<ssr_process_lease::Model, LeaseError> {
    let transaction = db.begin().await.map_err(db_error)?;
    let lease = ssr_process_lease::Entity::find_by_id(lease_id)
        .filter(ssr_process_lease::Column::DeploymentId.eq(deployment_id))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(db_error)?
        .ok_or(LeaseError::NotFound)?;
    if lease.node_id != node_id {
        return Err(LeaseError::WrongNode);
    }
    if lease.released_at.is_some() || lease.expires_at <= now {
        return Err(LeaseError::NotFound);
    }
    let next_block = if now >= lease.hour_block_start + HOUR_BLOCK {
        let deployment = deployment::Entity::find_by_id(deployment_id)
            .filter(deployment::Column::DeletedAt.is_null())
            .one(&transaction)
            .await
            .map_err(db_error)?
            .ok_or(LeaseError::NotFound)?;
        let team = team::Entity::find_by_id(deployment.team_id)
            .filter(team::Column::DeletedAt.is_null())
            .one(&transaction)
            .await
            .map_err(db_error)?
            .ok_or(LeaseError::NotFound)?;
        let resolved = quotas::resolve_team_quota(&transaction, &team)
            .await
            .map_err(db_error)?;
        let usage = quotas::effective_usage(&transaction, team.id, QuotaDimension::SsrHoursMonthly)
            .await
            .map_err(db_error)?;
        if resolved
            .limit_for(QuotaDimension::SsrHoursMonthly)
            .is_some_and(|limit| usage >= limit)
        {
            return Err(LeaseError::HourQuota);
        }
        Some(lease.hour_block_start + HOUR_BLOCK)
    } else {
        None
    };
    let mut active: ssr_process_lease::ActiveModel = lease.into();
    active.renewed_at = Set(now);
    active.expires_at = Set(now + LEASE_TTL);
    if let Some(block) = next_block {
        active.hour_block_start = Set(block);
    }
    let updated = active.update(&transaction).await.map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    if next_block.is_some() {
        record_block(db, &updated, now).await?;
    }
    Ok(updated)
}

pub async fn release(
    db: &sea_orm::DatabaseConnection,
    lease_id: Uuid,
    deployment_id: Uuid,
    node_id: Uuid,
    now: OffsetDateTime,
) -> Result<bool, LeaseError> {
    let transaction = db.begin().await.map_err(db_error)?;
    let Some(lease) = ssr_process_lease::Entity::find_by_id(lease_id)
        .filter(ssr_process_lease::Column::DeploymentId.eq(deployment_id))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(db_error)?
    else {
        return Ok(false);
    };
    if lease.node_id != node_id {
        return Err(LeaseError::WrongNode);
    }
    if lease.released_at.is_some() {
        transaction.commit().await.map_err(db_error)?;
        return Ok(false);
    }
    let mut active: ssr_process_lease::ActiveModel = lease.clone().into();
    active.released_at = Set(Some(now));
    active.update(&transaction).await.map_err(db_error)?;
    transaction.commit().await.map_err(db_error)?;
    release_process_event(db, &lease).await?;
    if now < lease.hour_block_start + HOUR_BLOCK {
        release_block_event(db, &lease).await?;
    }
    Ok(true)
}

pub async fn release_expired(
    db: &sea_orm::DatabaseConnection,
    now: OffsetDateTime,
) -> anyhow::Result<u64> {
    let leases = ssr_process_lease::Entity::find()
        .filter(ssr_process_lease::Column::ReleasedAt.is_null())
        .filter(ssr_process_lease::Column::ExpiresAt.lte(now))
        .all(db)
        .await?;
    let mut released = 0;
    for lease in leases {
        if release(db, lease.id, lease.deployment_id, lease.node_id, now)
            .await
            .is_ok()
        {
            released += 1;
        }
    }
    Ok(released)
}

async fn record_block(
    db: &sea_orm::DatabaseConnection,
    lease: &ssr_process_lease::Model,
    now: OffsetDateTime,
) -> Result<(), LeaseError> {
    quotas::record_event_once(
        db,
        format!(
            "ssr-hour:{}:{}",
            lease.id,
            lease.hour_block_start.unix_timestamp()
        ),
        RecordEventParams {
            team_id: lease.team_id,
            dimension: QuotaDimension::SsrHoursMonthly,
            kind: QuotaEventKind::Consume,
            delta_value: 1,
            resource_type: Some("ssr_process_lease".to_owned()),
            resource_id: Some(lease.id),
            metadata: serde_json::json!({ "hour_block_start": lease.hour_block_start, "at": now }),
        },
    )
    .await
    .map(|_| ())
    .map_err(db_error)
}

async fn release_process_event(
    db: &sea_orm::DatabaseConnection,
    lease: &ssr_process_lease::Model,
) -> Result<(), LeaseError> {
    quotas::record_event_once(
        db,
        format!("ssr-process-release:{}", lease.id),
        RecordEventParams {
            team_id: lease.team_id,
            dimension: QuotaDimension::SsrProcesses,
            kind: QuotaEventKind::Release,
            delta_value: -1,
            resource_type: Some("ssr_process_lease".to_owned()),
            resource_id: Some(lease.id),
            metadata: serde_json::json!({}),
        },
    )
    .await
    .map(|_| ())
    .map_err(db_error)
}

async fn record_process_event(
    db: &sea_orm::DatabaseConnection,
    lease: &ssr_process_lease::Model,
) -> Result<(), LeaseError> {
    quotas::record_event_once(
        db,
        format!("ssr-process:{}", lease.id),
        RecordEventParams {
            team_id: lease.team_id,
            dimension: QuotaDimension::SsrProcesses,
            kind: QuotaEventKind::Consume,
            delta_value: 1,
            resource_type: Some("ssr_process_lease".to_owned()),
            resource_id: Some(lease.id),
            metadata: serde_json::json!({}),
        },
    )
    .await
    .map(|_| ())
    .map_err(db_error)
}

async fn release_block_event(
    db: &sea_orm::DatabaseConnection,
    lease: &ssr_process_lease::Model,
) -> Result<(), LeaseError> {
    quotas::record_event_once(
        db,
        format!(
            "ssr-hour-release:{}:{}",
            lease.id,
            lease.hour_block_start.unix_timestamp()
        ),
        RecordEventParams {
            team_id: lease.team_id,
            dimension: QuotaDimension::SsrHoursMonthly,
            kind: QuotaEventKind::Release,
            delta_value: -1,
            resource_type: Some("ssr_process_lease".to_owned()),
            resource_id: Some(lease.id),
            metadata: serde_json::json!({}),
        },
    )
    .await
    .map(|_| ())
    .map_err(db_error)
}

fn db_error(error: impl Into<anyhow::Error>) -> LeaseError {
    LeaseError::Database(error.into())
}
