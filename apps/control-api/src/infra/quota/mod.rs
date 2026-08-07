//! Quota enforcement orchestration.
//!
//! Fast-path counters live in the cache store (Redis in production, in-memory
//! fallback otherwise) and are checked plus pre-consumed atomically. Durable
//! truth lives in PostgreSQL: `quota_events` records every change and
//! `quota_usage_counters` aggregates usage for precise reads and counter
//! rebuilds. Cache keys are lazily seeded from the durable counters so a cold
//! cache converges back to the database state.

use std::time::Duration;

use grass_cache::{Cache, CacheStore, QuotaCheckOutcome, QuotaCounterCheck};
use sea_orm::DatabaseConnection;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        quotas::{self, QuotaDimension, RecordEventParams, ResolvedQuota},
    },
    infra::{
        database::entity::{AuditEventResult, QuotaEventKind, QuotaPeriod, team},
        error::AppError,
    },
};

/// Non-periodic counter keys survive this long before being reseeded from
/// the durable counters.
const STATIC_COUNTER_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 30);
/// Monthly counter keys outlive their window slightly so late rollbacks can
/// still find them.
const MONTHLY_COUNTER_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 40);
/// A concurrent-build slot must be refreshed by the heartbeat of the running
/// build; expired slots free themselves after a crashed Node stops renewing.
#[allow(dead_code)] // Wired by the Node claim flow in Milestone 6.
pub const BUILD_SLOT_TTL: Duration = Duration::from_secs(60 * 30);

pub struct QuotaCharge {
    pub dimension: QuotaDimension,
    pub amount: i64,
}

impl QuotaCharge {
    pub fn one(dimension: QuotaDimension) -> Self {
        Self {
            dimension,
            amount: 1,
        }
    }

    #[allow(dead_code)] // Wired by build-minute and storage charges in Milestone 7.
    pub fn amount(dimension: QuotaDimension, amount: i64) -> Self {
        Self { dimension, amount }
    }
}

/// A successful reservation that must be either committed (business success)
/// or rolled back (business failure) so cache counters stay aligned.
pub struct QuotaReservation {
    team_id: Uuid,
    entries: Vec<(QuotaDimension, i64, String)>,
}

pub struct QuotaService<'a> {
    db: &'a DatabaseConnection,
    cache: &'a CacheStore,
}

impl<'a> QuotaService<'a> {
    pub fn new(db: &'a DatabaseConnection, cache: &'a CacheStore) -> Self {
        Self { db, cache }
    }

    /// Atomically checks and pre-consumes the requested amounts for a team.
    /// On denial, a deny quota event plus an audit event are recorded and a
    /// stable `QuotaExceeded` error is returned.
    pub async fn reserve(
        &self,
        op: &'static str,
        team: &team::Model,
        actor_user_id: Option<Uuid>,
        charges: &[QuotaCharge],
    ) -> Result<QuotaReservation, AppError> {
        let resolved = quotas::resolve_team_quota(self.db, team)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

        let mut checks = Vec::with_capacity(charges.len());
        let mut entries = Vec::with_capacity(charges.len());
        for charge in charges {
            let key = counter_key(team.id, charge.dimension);
            self.seed_counter(team.id, charge.dimension, &key)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;

            let max = resolved.limit_for(charge.dimension).unwrap_or(-1);
            checks.push(QuotaCounterCheck {
                key: key.clone(),
                amount: charge.amount,
                max,
                ttl: Some(counter_ttl(charge.dimension)),
            });
            entries.push((charge.dimension, charge.amount, key));
        }

        let outcome = self
            .cache
            .check_and_consume(&checks)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

        match outcome {
            QuotaCheckOutcome::Allowed => Ok(QuotaReservation {
                team_id: team.id,
                entries,
            }),
            QuotaCheckOutcome::Denied { key } => {
                let denied = entries
                    .iter()
                    .find(|(_, _, entry_key)| *entry_key == key)
                    .map(|(dimension, _, _)| *dimension)
                    .unwrap_or(QuotaDimension::Projects);
                self.record_denial(op, team.id, actor_user_id, denied, &resolved)
                    .await;
                Err(quota_exceeded_error(op, denied))
            }
        }
    }

    /// Confirms a reservation after the business operation succeeded by
    /// writing consume events and durable counters.
    pub async fn commit(
        &self,
        op: &'static str,
        reservation: QuotaReservation,
        resource_type: &str,
        resource_id: Option<Uuid>,
    ) -> Result<(), AppError> {
        for (dimension, amount, _) in &reservation.entries {
            quotas::record_event(
                self.db,
                RecordEventParams {
                    team_id: reservation.team_id,
                    dimension: *dimension,
                    kind: QuotaEventKind::Consume,
                    delta_value: *amount,
                    resource_type: Some(resource_type.to_owned()),
                    resource_id,
                    metadata: serde_json::json!({}),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        }
        Ok(())
    }

    /// Rolls a reservation back after the business operation failed.
    pub async fn rollback(&self, reservation: QuotaReservation) {
        for (_, amount, key) in &reservation.entries {
            if let Err(error) = self.cache.adjust_counter(key, -amount).await {
                tracing::warn!(
                    operation = "quota.rollback",
                    %error,
                    key = %key,
                    "failed to roll back reserved quota counter"
                );
            }
        }
    }

    /// Records usage that already happened and must not be blocked by the
    /// limit, such as build minutes measured after a build finished. The
    /// cache counter still advances so subsequent limit checks see it.
    pub async fn charge_unchecked(
        &self,
        op: &'static str,
        team_id: Uuid,
        charges: &[QuotaCharge],
        resource_type: &str,
        resource_id: Option<Uuid>,
    ) -> Result<(), AppError> {
        for charge in charges {
            let key = counter_key(team_id, charge.dimension);
            self.cache
                .adjust_counter(&key, charge.amount)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;
            quotas::record_event(
                self.db,
                RecordEventParams {
                    team_id,
                    dimension: charge.dimension,
                    kind: QuotaEventKind::Consume,
                    delta_value: charge.amount,
                    resource_type: Some(resource_type.to_owned()),
                    resource_id,
                    metadata: serde_json::json!({ "unchecked": true }),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        }
        Ok(())
    }

    /// Releases previously consumed usage (project deleted, artifact removed,
    /// member removed) in both the cache and durable counters.
    pub async fn release(
        &self,
        op: &'static str,
        team_id: Uuid,
        charges: &[QuotaCharge],
        resource_type: &str,
        resource_id: Option<Uuid>,
    ) -> Result<(), AppError> {
        for charge in charges {
            let key = counter_key(team_id, charge.dimension);
            self.cache
                .adjust_counter(&key, -charge.amount)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;
            quotas::record_event(
                self.db,
                RecordEventParams {
                    team_id,
                    dimension: charge.dimension,
                    kind: QuotaEventKind::Release,
                    delta_value: -charge.amount,
                    resource_type: Some(resource_type.to_owned()),
                    resource_id,
                    metadata: serde_json::json!({}),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        }
        Ok(())
    }

    /// Releases a resource's usage once in durable storage, then rebuilds
    /// the corresponding cache counter. Repeating the same call is safe and
    /// also repairs a cache update that failed after the durable commit.
    pub async fn release_once(
        &self,
        op: &'static str,
        team_id: Uuid,
        charges: &[QuotaCharge],
        resource_type: &str,
        resource_id: Uuid,
    ) -> Result<(), AppError> {
        self.release_once_with_key(
            op,
            team_id,
            charges,
            resource_type,
            resource_id,
            |dimension| release_idempotency_key(resource_type, resource_id, dimension),
        )
        .await
    }

    /// Releases a project resource once for one soft-deletion generation.
    /// A later delete after restore receives a distinct key while retries of
    /// the same tombstone remain idempotent.
    pub async fn release_once_for_generation(
        &self,
        op: &'static str,
        team_id: Uuid,
        charges: &[QuotaCharge],
        resource_type: &str,
        resource_id: Uuid,
        generation: OffsetDateTime,
    ) -> Result<(), AppError> {
        self.release_once_with_key(
            op,
            team_id,
            charges,
            resource_type,
            resource_id,
            |dimension| {
                release_idempotency_key_for_generation(
                    resource_type,
                    resource_id,
                    dimension,
                    generation,
                )
            },
        )
        .await
    }

    async fn release_once_with_key<F>(
        &self,
        op: &'static str,
        team_id: Uuid,
        charges: &[QuotaCharge],
        resource_type: &str,
        resource_id: Uuid,
        key_for_dimension: F,
    ) -> Result<(), AppError>
    where
        F: Fn(QuotaDimension) -> String,
    {
        for charge in charges {
            quotas::record_event_once(
                self.db,
                key_for_dimension(charge.dimension),
                RecordEventParams {
                    team_id,
                    dimension: charge.dimension,
                    kind: QuotaEventKind::Release,
                    delta_value: -charge.amount,
                    resource_type: Some(resource_type.to_owned()),
                    resource_id: Some(resource_id),
                    metadata: serde_json::json!({ "idempotent": true }),
                },
            )
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

            let usage = quotas::effective_usage(self.db, team_id, charge.dimension)
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;
            self.cache
                .set(
                    &counter_key(team_id, charge.dimension),
                    &usage.to_string(),
                    counter_ttl(charge.dimension),
                )
                .await
                .map_err(|source| AppError::Infrastructure { op, source })?;
        }
        Ok(())
    }

    /// Reads a scalar (non-counted) limit such as the build timeout or the
    /// per-artifact size limit. `None` means unlimited.
    #[allow(dead_code)] // Wired by build and artifact limits in Milestones 6 and 7.
    pub async fn scalar_limit(
        &self,
        op: &'static str,
        team: &team::Model,
        dimension: QuotaDimension,
    ) -> Result<Option<i64>, AppError> {
        let resolved = quotas::resolve_team_quota(self.db, team)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        Ok(resolved.limit_for(dimension))
    }

    /// Acquires one concurrent-build slot for a team. Returns whether the
    /// slot was acquired. Slots expire after [`BUILD_SLOT_TTL`] unless
    /// refreshed, so crashed Nodes cannot pin slots forever.
    #[allow(dead_code)] // Wired by the Node claim flow in Milestone 6.
    pub async fn acquire_build_slot(
        &self,
        op: &'static str,
        team: &team::Model,
    ) -> Result<bool, AppError> {
        let resolved = quotas::resolve_team_quota(self.db, team)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;
        let max = resolved
            .limit_for(QuotaDimension::ConcurrentBuilds)
            .unwrap_or(-1);
        let key = slot_key(team.id);
        let acquired = self
            .cache
            .acquire_slot(&key, max, BUILD_SLOT_TTL)
            .await
            .map_err(|source| AppError::Infrastructure { op, source })?;

        if !acquired {
            self.record_denial(
                op,
                team.id,
                None,
                QuotaDimension::ConcurrentBuilds,
                &resolved,
            )
            .await;
        }
        Ok(acquired)
    }

    #[allow(dead_code)] // Wired by the Node claim flow in Milestone 6.
    pub async fn release_build_slot(&self, team_id: Uuid) {
        if let Err(error) = self.cache.release_slot(&slot_key(team_id)).await {
            tracing::warn!(
                operation = "quota.release_build_slot",
                %error,
                team_id = %team_id,
                "failed to release concurrent build slot"
            );
        }
    }

    /// Releases the slot owned by one deployment at most once. The marker is
    /// atomic in both cache backends, so a user cancel racing a terminal Node
    /// report cannot decrement the team's counter twice.
    pub async fn release_build_slot_once(&self, team_id: Uuid, deployment_id: Uuid) {
        if let Err(error) = release_build_slot_once(self.cache, team_id, deployment_id).await {
            tracing::warn!(
                operation = "quota.release_build_slot_once",
                %error,
                team_id = %team_id,
                deployment_id = %deployment_id,
                "failed to release concurrent build slot"
            );
        }
    }

    /// Refreshes the TTL of a team's build-slot counter while a build is
    /// still running.
    #[allow(dead_code)] // Wired by the Node stage flow in Milestone 6.
    pub async fn refresh_build_slot(&self, team_id: Uuid) {
        let key = slot_key(team_id);
        if let Ok(Some(value)) = self.cache.get(&key).await {
            let _ = self
                .cache
                .update_if_present(&key, &value, BUILD_SLOT_TTL)
                .await;
        }
    }

    /// Rebuilds a team's cache counters from the durable usage counters.
    #[allow(dead_code)] // Wired by the calibration task in Milestone 6.
    pub async fn recalibrate_team(&self, team_id: Uuid) -> anyhow::Result<()> {
        for dimension in QuotaDimension::ALL {
            if !dimension.is_counted() {
                continue;
            }
            let usage = quotas::effective_usage(self.db, team_id, *dimension).await?;
            let key = counter_key(team_id, *dimension);
            self.cache
                .set(&key, &usage.to_string(), counter_ttl(*dimension))
                .await?;
        }
        Ok(())
    }

    async fn seed_counter(
        &self,
        team_id: Uuid,
        dimension: QuotaDimension,
        key: &str,
    ) -> anyhow::Result<()> {
        if !dimension.is_counted() {
            return Ok(());
        }
        if self.cache.get(key).await?.is_some() {
            return Ok(());
        }
        let usage = quotas::effective_usage(self.db, team_id, dimension).await?;
        self.cache
            .set_if_absent(key, &usage.to_string(), counter_ttl(dimension))
            .await?;
        Ok(())
    }

    async fn record_denial(
        &self,
        op: &'static str,
        team_id: Uuid,
        actor_user_id: Option<Uuid>,
        dimension: QuotaDimension,
        resolved: &ResolvedQuota,
    ) {
        tracing::warn!(
            operation = op,
            team_id = %team_id,
            dimension = dimension.as_str(),
            "quota denied"
        );

        if let Err(error) = quotas::record_event(
            self.db,
            RecordEventParams {
                team_id,
                dimension,
                kind: QuotaEventKind::Deny,
                delta_value: 0,
                resource_type: None,
                resource_id: None,
                metadata: serde_json::json!({
                    "op": op,
                    "plan": resolved.plan.code,
                }),
            },
        )
        .await
        {
            tracing::warn!(operation = op, %error, "failed to record quota deny event");
        }

        if let Err(error) = audits::create_audit_event(
            self.db,
            CreateAuditEventParams {
                actor_user_id,
                actor_node_id: None,
                team_id: Some(team_id),
                action: "quota.denied".to_owned(),
                target_type: "team".to_owned(),
                target_id: Some(team_id),
                result: AuditEventResult::Denied,
                reason: Some(format!("{} limit reached", dimension.as_str())),
                metadata: serde_json::json!({
                    "op": op,
                    "dimension": dimension.as_str(),
                    "plan": resolved.plan.code,
                }),
            },
        )
        .await
        {
            tracing::warn!(operation = op, %error, "failed to record quota audit event");
        }
    }
}

pub fn quota_exceeded_error(op: &'static str, dimension: QuotaDimension) -> AppError {
    AppError::QuotaExceeded {
        op,
        message: format!("quota exceeded: {} limit reached", dimension.as_str()),
    }
}

fn counter_key(team_id: Uuid, dimension: QuotaDimension) -> String {
    match dimension.period() {
        QuotaPeriod::Monthly => {
            let now = time::OffsetDateTime::now_utc();
            format!(
                "quota:team:{team_id}:{}:{:04}{:02}",
                dimension.as_str(),
                now.year(),
                u8::from(now.month())
            )
        }
        QuotaPeriod::None => format!("quota:team:{team_id}:{}", dimension.as_str()),
    }
}

#[allow(dead_code)] // Wired by the Node claim flow in Milestone 6.
fn slot_key(team_id: Uuid) -> String {
    format!("quota:team:{team_id}:concurrent_builds")
}

fn slot_release_key(deployment_id: Uuid) -> String {
    format!("quota:deployment:{deployment_id}:build_slot_released")
}

fn release_idempotency_key(
    resource_type: &str,
    resource_id: Uuid,
    dimension: QuotaDimension,
) -> String {
    format!(
        "release:{resource_type}:{resource_id}:{}",
        dimension.as_str()
    )
}

fn release_idempotency_key_for_generation(
    resource_type: &str,
    resource_id: Uuid,
    dimension: QuotaDimension,
    generation: OffsetDateTime,
) -> String {
    format!(
        "release:{resource_type}:{resource_id}:{}:generation:{}",
        dimension.as_str(),
        generation.unix_timestamp_nanos()
    )
}

async fn release_build_slot_once(
    cache: &CacheStore,
    team_id: Uuid,
    deployment_id: Uuid,
) -> anyhow::Result<bool> {
    let owns_release = cache
        .set_if_absent(&slot_release_key(deployment_id), "1", STATIC_COUNTER_TTL)
        .await?;
    if owns_release {
        cache.release_slot(&slot_key(team_id)).await?;
    }
    Ok(owns_release)
}

fn counter_ttl(dimension: QuotaDimension) -> Duration {
    match dimension.period() {
        QuotaPeriod::Monthly => MONTHLY_COUNTER_TTL,
        QuotaPeriod::None => STATIC_COUNTER_TTL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_counter_keys_include_the_period_window() {
        let team_id = Uuid::nil();
        let key = counter_key(team_id, QuotaDimension::DeploymentsMonthly);
        assert!(key.starts_with(&format!("quota:team:{team_id}:deployments.monthly:")));
        assert_eq!(
            counter_key(team_id, QuotaDimension::Projects),
            format!("quota:team:{team_id}:projects")
        );
    }

    #[test]
    fn quota_exceeded_errors_carry_a_stable_message() {
        let error = quota_exceeded_error("test.quota", QuotaDimension::DeploymentsMonthly);
        assert_eq!(
            error.to_string(),
            "quota exceeded: deployments.monthly limit reached"
        );
    }

    #[test]
    fn resource_release_idempotency_keys_are_dimension_scoped() {
        let resource_id = Uuid::now_v7();
        assert_eq!(
            release_idempotency_key("project_host_binding", resource_id, QuotaDimension::Hosts),
            format!("release:project_host_binding:{resource_id}:hosts")
        );
    }

    #[test]
    fn project_release_idempotency_keys_include_the_deletion_generation() {
        let project_id = Uuid::now_v7();
        let generation = time::OffsetDateTime::from_unix_timestamp_nanos(1_234).unwrap();
        let same_generation = release_idempotency_key_for_generation(
            "project",
            project_id,
            QuotaDimension::Projects,
            generation,
        );
        assert_eq!(
            same_generation,
            release_idempotency_key_for_generation(
                "project",
                project_id,
                QuotaDimension::Projects,
                generation,
            )
        );
        assert_ne!(
            same_generation,
            release_idempotency_key_for_generation(
                "project",
                project_id,
                QuotaDimension::Projects,
                generation + time::Duration::nanoseconds(1),
            )
        );
    }

    #[tokio::test]
    async fn deployment_slot_release_is_atomic() {
        let cache = CacheStore::Moka(grass_cache::MokaCache::connect());
        let team_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let key = slot_key(team_id);
        assert!(cache.acquire_slot(&key, 10, BUILD_SLOT_TTL).await.unwrap());
        assert!(cache.acquire_slot(&key, 10, BUILD_SLOT_TTL).await.unwrap());

        let (first, second) = tokio::join!(
            release_build_slot_once(&cache, team_id, deployment_id),
            release_build_slot_once(&cache, team_id, deployment_id),
        );
        assert_ne!(first.unwrap(), second.unwrap());
        assert_eq!(cache.get(&key).await.unwrap().as_deref(), Some("1"));
    }
}
