//! Database-backed quota business functions.
//!
//! Quota plans define limits per dimension. Teams resolve to a plan through
//! an explicit override, their team group, or the platform default plan.
//! Durable usage lives in `quota_usage_counters`, and every consumption or
//! release is recorded in `quota_events` so counters can be rebuilt.

use std::collections::HashMap;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{
    QuotaEventKind, QuotaPeriod, quota_event, quota_limit, quota_plan, team, team_group,
};

/// First-stage quota dimensions. String values are stable identifiers stored
/// in `quota_limits.dimension`, `quota_usage_counters.dimension`, and
/// `quota_events.dimension`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuotaDimension {
    Projects,
    ProjectsStatic,
    ProjectsSsr,
    Members,
    Hosts,
    DeploymentsMonthly,
    BuildMinutesMonthly,
    BuildTimeoutSeconds,
    StorageMb,
    ArtifactMaxMb,
    ConcurrentBuilds,
    SsrProcesses,
    SsrHoursMonthly,
}

impl QuotaDimension {
    pub const ALL: &'static [Self] = &[
        Self::Projects,
        Self::ProjectsStatic,
        Self::ProjectsSsr,
        Self::Members,
        Self::Hosts,
        Self::DeploymentsMonthly,
        Self::BuildMinutesMonthly,
        Self::BuildTimeoutSeconds,
        Self::StorageMb,
        Self::ArtifactMaxMb,
        Self::ConcurrentBuilds,
        Self::SsrProcesses,
        Self::SsrHoursMonthly,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::ProjectsStatic => "projects.static",
            Self::ProjectsSsr => "projects.ssr",
            Self::Members => "members",
            Self::Hosts => "hosts",
            Self::DeploymentsMonthly => "deployments.monthly",
            Self::BuildMinutesMonthly => "build_minutes.monthly",
            Self::BuildTimeoutSeconds => "build_timeout_seconds",
            Self::StorageMb => "storage_mb",
            Self::ArtifactMaxMb => "artifact_max_mb",
            Self::ConcurrentBuilds => "concurrent_builds",
            Self::SsrProcesses => "ssr_processes",
            Self::SsrHoursMonthly => "ssr_hours.monthly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|dimension| dimension.as_str() == value)
    }

    pub fn period(&self) -> QuotaPeriod {
        match self {
            Self::DeploymentsMonthly | Self::BuildMinutesMonthly | Self::SsrHoursMonthly => {
                QuotaPeriod::Monthly
            }
            _ => QuotaPeriod::None,
        }
    }

    /// Dimensions that behave as running counters. Scalar limits such as the
    /// per-build timeout and per-artifact size are enforced at operation time
    /// and never accumulate usage.
    pub fn is_counted(&self) -> bool {
        !matches!(
            self,
            Self::BuildTimeoutSeconds | Self::ArtifactMaxMb | Self::ConcurrentBuilds
        )
    }
}

/// Where the resolved quota plan came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaPlanSource {
    Explicit,
    Group,
    Default,
}

impl QuotaPlanSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Group => "group",
            Self::Default => "default",
        }
    }
}

pub struct ResolvedQuota {
    pub plan: quota_plan::Model,
    pub source: QuotaPlanSource,
    pub limits: HashMap<String, i64>,
}

impl ResolvedQuota {
    /// Returns the limit for a dimension. A missing limit row or a negative
    /// stored value both mean unlimited, expressed as `None`.
    pub fn limit_for(&self, dimension: QuotaDimension) -> Option<i64> {
        self.limits
            .get(dimension.as_str())
            .copied()
            .filter(|limit| *limit >= 0)
    }
}

/// Resolves the effective quota plan and limits for a team: an explicit team
/// override wins over the team group plan, which wins over the default plan.
pub async fn resolve_team_quota<C: ConnectionTrait>(
    db: &C,
    team: &team::Model,
) -> anyhow::Result<ResolvedQuota> {
    let (plan, source) = if let Some(plan_id) = team.explicit_quota_plan_id {
        let plan = plan_by_id(db, plan_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("explicit quota plan {plan_id} not found"))?;
        (plan, QuotaPlanSource::Explicit)
    } else if let Some(group_plan) = group_plan(db, team.group_id).await? {
        (group_plan, QuotaPlanSource::Group)
    } else {
        let plan = default_plan(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no default quota plan is configured"))?;
        (plan, QuotaPlanSource::Default)
    };

    let limits = quota_limit::Entity::find()
        .filter(quota_limit::Column::QuotaPlanId.eq(plan.id))
        .all(db)
        .await?
        .into_iter()
        .map(|limit| (limit.dimension, limit.limit_value))
        .collect();

    Ok(ResolvedQuota {
        plan,
        source,
        limits,
    })
}

async fn plan_by_id<C: ConnectionTrait>(
    db: &C,
    plan_id: Uuid,
) -> anyhow::Result<Option<quota_plan::Model>> {
    quota_plan::Entity::find()
        .filter(quota_plan::Column::Id.eq(plan_id))
        .filter(quota_plan::Column::Enabled.eq(true))
        .one(db)
        .await
        .map_err(Into::into)
}

async fn group_plan<C: ConnectionTrait>(
    db: &C,
    group_id: Option<Uuid>,
) -> anyhow::Result<Option<quota_plan::Model>> {
    let Some(group_id) = group_id else {
        return Ok(None);
    };
    let Some(group) = team_group::Entity::find()
        .filter(team_group::Column::Id.eq(group_id))
        .filter(team_group::Column::DeletedAt.is_null())
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let Some(plan_id) = group.quota_plan_id else {
        return Ok(None);
    };
    plan_by_id(db, plan_id).await
}

async fn default_plan<C: ConnectionTrait>(db: &C) -> anyhow::Result<Option<quota_plan::Model>> {
    quota_plan::Entity::find()
        .filter(quota_plan::Column::IsDefault.eq(true))
        .filter(quota_plan::Column::Enabled.eq(true))
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn list_plans<C: ConnectionTrait>(
    db: &C,
) -> anyhow::Result<Vec<(quota_plan::Model, Vec<quota_limit::Model>)>> {
    let plans = quota_plan::Entity::find().all(db).await?;
    let limits = quota_limit::Entity::find().all(db).await?;

    Ok(plans
        .into_iter()
        .map(|plan| {
            let plan_limits = limits
                .iter()
                .filter(|limit| limit.quota_plan_id == plan.id)
                .cloned()
                .collect();
            (plan, plan_limits)
        })
        .collect())
}

pub struct CreatePlanParams {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub limits: Vec<(QuotaDimension, i64)>,
}

pub async fn create_plan<C: ConnectionTrait>(
    db: &C,
    params: CreatePlanParams,
) -> anyhow::Result<quota_plan::Model> {
    let now = OffsetDateTime::now_utc();
    let plan = quota_plan::ActiveModel {
        id: Set(Uuid::now_v7()),
        code: Set(params.code),
        name: Set(params.name),
        description: Set(params.description),
        is_default: Set(false),
        enabled: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?;

    for (dimension, limit_value) in params.limits {
        upsert_limit(db, plan.id, dimension, limit_value).await?;
    }

    Ok(plan)
}

pub struct UpdatePlanParams {
    pub name: Option<String>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub limits: Vec<(QuotaDimension, i64)>,
}

pub async fn update_plan<C: ConnectionTrait>(
    db: &C,
    plan_id: Uuid,
    params: UpdatePlanParams,
) -> anyhow::Result<Option<quota_plan::Model>> {
    let Some(plan) = quota_plan::Entity::find()
        .filter(quota_plan::Column::Id.eq(plan_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    let mut active: quota_plan::ActiveModel = plan.into();
    if let Some(name) = params.name {
        active.name = Set(name);
    }
    if let Some(description) = params.description {
        active.description = Set(Some(description));
    }
    if let Some(enabled) = params.enabled {
        active.enabled = Set(enabled);
    }
    let plan = active.update(db).await?;

    for (dimension, limit_value) in params.limits {
        upsert_limit(db, plan.id, dimension, limit_value).await?;
    }

    Ok(Some(plan))
}

async fn upsert_limit<C: ConnectionTrait>(
    db: &C,
    plan_id: Uuid,
    dimension: QuotaDimension,
    limit_value: i64,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let existing = quota_limit::Entity::find()
        .filter(quota_limit::Column::QuotaPlanId.eq(plan_id))
        .filter(quota_limit::Column::Dimension.eq(dimension.as_str()))
        .one(db)
        .await?;

    match existing {
        Some(limit) => {
            let mut active: quota_limit::ActiveModel = limit.into();
            active.limit_value = Set(limit_value);
            active.update(db).await?;
        }
        None => {
            quota_limit::ActiveModel {
                id: Set(Uuid::now_v7()),
                quota_plan_id: Set(plan_id),
                dimension: Set(dimension.as_str().to_owned()),
                limit_value: Set(limit_value),
                period: Set(dimension.period()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
    }

    Ok(())
}

/// Current monthly window boundaries in UTC.
pub fn current_period_window(now: OffsetDateTime) -> (OffsetDateTime, OffsetDateTime) {
    let start = now
        .replace_day(1)
        .expect("day 1 is always valid")
        .replace_time(time::Time::MIDNIGHT);
    let (next_year, next_month) = match start.month() {
        time::Month::December => (start.year() + 1, time::Month::January),
        month => (start.year(), month.next()),
    };
    let end = start
        .replace_year(next_year)
        .and_then(|d| d.replace_month(next_month))
        .expect("first day of next month is always valid");
    (start, end)
}

pub struct RecordEventParams {
    pub team_id: Uuid,
    pub dimension: QuotaDimension,
    pub kind: QuotaEventKind,
    pub delta_value: i64,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

/// Writes a quota event and folds the delta into the matching durable usage
/// counter. Deny events are recorded without touching counters.
pub async fn record_event<C: ConnectionTrait>(
    db: &C,
    params: RecordEventParams,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    quota_event::ActiveModel {
        id: Set(Uuid::now_v7()),
        team_id: Set(params.team_id),
        dimension: Set(params.dimension.as_str().to_owned()),
        kind: Set(params.kind.clone()),
        delta_value: Set(params.delta_value),
        idempotency_key: Set(None),
        resource_type: Set(params.resource_type),
        resource_id: Set(params.resource_id),
        metadata: Set(params.metadata),
        created_at: Set(now),
    }
    .insert(db)
    .await?;

    if matches!(
        params.kind,
        QuotaEventKind::Consume | QuotaEventKind::Release | QuotaEventKind::Adjust
    ) {
        apply_counter_delta(
            db,
            params.team_id,
            params.dimension,
            params.delta_value,
            now,
        )
        .await?;
    }

    Ok(())
}

async fn apply_counter_delta<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    dimension: QuotaDimension,
    delta: i64,
    now: OffsetDateTime,
) -> anyhow::Result<()> {
    use crate::infra::database::entity::quota_usage_counter;

    let (period_start, period_end) = match dimension.period() {
        QuotaPeriod::Monthly => {
            let (start, end) = current_period_window(now);
            (Some(start), Some(end))
        }
        QuotaPeriod::None => (None, None),
    };

    let mut query = quota_usage_counter::Entity::find()
        .filter(quota_usage_counter::Column::TeamId.eq(team_id))
        .filter(quota_usage_counter::Column::Dimension.eq(dimension.as_str()));
    query = match period_start {
        Some(start) => query.filter(quota_usage_counter::Column::PeriodStart.eq(start)),
        None => query.filter(quota_usage_counter::Column::PeriodStart.is_null()),
    };

    match query.one(db).await? {
        Some(counter) => {
            let next = (counter.used_value + delta).max(0);
            let mut active: quota_usage_counter::ActiveModel = counter.into();
            active.used_value = Set(next);
            active.update(db).await?;
        }
        None => {
            quota_usage_counter::ActiveModel {
                id: Set(Uuid::now_v7()),
                team_id: Set(team_id),
                dimension: Set(dimension.as_str().to_owned()),
                used_value: Set(delta.max(0)),
                period_start: Set(period_start),
                period_end: Set(period_end),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
    }

    Ok(())
}

/// Reads the durable usage for one dimension, scoped to the current monthly
/// window for periodic dimensions.
pub async fn durable_usage<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    dimension: QuotaDimension,
) -> anyhow::Result<i64> {
    use crate::infra::database::entity::quota_usage_counter;

    let mut query = quota_usage_counter::Entity::find()
        .filter(quota_usage_counter::Column::TeamId.eq(team_id))
        .filter(quota_usage_counter::Column::Dimension.eq(dimension.as_str()));
    query = match dimension.period() {
        QuotaPeriod::Monthly => {
            let (start, _) = current_period_window(OffsetDateTime::now_utc());
            query.filter(quota_usage_counter::Column::PeriodStart.eq(start))
        }
        QuotaPeriod::None => query.filter(quota_usage_counter::Column::PeriodStart.is_null()),
    };

    Ok(query.one(db).await?.map(|c| c.used_value).unwrap_or(0))
}

/// Reads the effective usage for one dimension. Structural dimensions count
/// live rows so the value stays truthful regardless of event history, while
/// periodic and storage dimensions read the durable counters aggregated from
/// quota events.
pub async fn effective_usage<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    dimension: QuotaDimension,
) -> anyhow::Result<i64> {
    use sea_orm::PaginatorTrait;

    use crate::infra::database::entity::{
        ProjectRuntime, project, project_host_binding, team_member,
    };

    let count = match dimension {
        QuotaDimension::Projects => {
            project::Entity::find()
                .filter(project::Column::TeamId.eq(team_id))
                .filter(project::Column::DeletedAt.is_null())
                .count(db)
                .await?
        }
        QuotaDimension::ProjectsStatic => {
            project::Entity::find()
                .filter(project::Column::TeamId.eq(team_id))
                .filter(project::Column::Runtime.eq(ProjectRuntime::Static))
                .filter(project::Column::DeletedAt.is_null())
                .count(db)
                .await?
        }
        QuotaDimension::ProjectsSsr => {
            project::Entity::find()
                .filter(project::Column::TeamId.eq(team_id))
                .filter(project::Column::Runtime.eq(ProjectRuntime::Ssr))
                .filter(project::Column::DeletedAt.is_null())
                .count(db)
                .await?
        }
        QuotaDimension::Members => {
            team_member::Entity::find()
                .filter(team_member::Column::TeamId.eq(team_id))
                .filter(team_member::Column::DeletedAt.is_null())
                .count(db)
                .await?
        }
        QuotaDimension::Hosts => {
            project_host_binding::Entity::find()
                .filter(project_host_binding::Column::TeamId.eq(team_id))
                .filter(project_host_binding::Column::DeletedAt.is_null())
                .count(db)
                .await?
        }
        _ => return durable_usage(db, team_id, dimension).await,
    };

    Ok(count as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn dimension_identifiers_round_trip() {
        for dimension in QuotaDimension::ALL {
            assert_eq!(QuotaDimension::parse(dimension.as_str()), Some(*dimension));
        }
        assert_eq!(QuotaDimension::parse("unknown"), None);
    }

    #[test]
    fn monthly_dimensions_use_monthly_period() {
        assert_eq!(
            QuotaDimension::DeploymentsMonthly.period(),
            QuotaPeriod::Monthly
        );
        assert_eq!(
            QuotaDimension::BuildMinutesMonthly.period(),
            QuotaPeriod::Monthly
        );
        assert_eq!(QuotaDimension::Projects.period(), QuotaPeriod::None);
    }

    #[test]
    fn scalar_limits_are_not_counted() {
        assert!(!QuotaDimension::BuildTimeoutSeconds.is_counted());
        assert!(!QuotaDimension::ArtifactMaxMb.is_counted());
        assert!(!QuotaDimension::ConcurrentBuilds.is_counted());
        assert!(QuotaDimension::Projects.is_counted());
        assert!(QuotaDimension::DeploymentsMonthly.is_counted());
    }

    #[test]
    fn period_window_spans_one_month() {
        let (start, end) = current_period_window(datetime!(2026-07-26 10:30 UTC));
        assert_eq!(start, datetime!(2026-07-01 0:00 UTC));
        assert_eq!(end, datetime!(2026-08-01 0:00 UTC));

        let (start, end) = current_period_window(datetime!(2026-12-15 23:59 UTC));
        assert_eq!(start, datetime!(2026-12-01 0:00 UTC));
        assert_eq!(end, datetime!(2027-01-01 0:00 UTC));
    }

    #[test]
    fn missing_or_negative_limits_mean_unlimited() {
        let resolved = ResolvedQuota {
            plan: quota_plan::Model {
                id: Uuid::nil(),
                code: "test".into(),
                name: "Test".into(),
                description: None,
                is_default: false,
                enabled: true,
                created_at: OffsetDateTime::UNIX_EPOCH,
                updated_at: OffsetDateTime::UNIX_EPOCH,
            },
            source: QuotaPlanSource::Default,
            limits: HashMap::from([("projects".to_owned(), 3), ("hosts".to_owned(), -1)]),
        };

        assert_eq!(resolved.limit_for(QuotaDimension::Projects), Some(3));
        assert_eq!(resolved.limit_for(QuotaDimension::Hosts), None);
        assert_eq!(resolved.limit_for(QuotaDimension::Members), None);
    }
}
