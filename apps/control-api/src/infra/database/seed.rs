//! Idempotent database seed entrypoint.
//!
//! The first-stage setup flow expects the database to contain baseline plans,
//! team groups, host policies, and review policy defaults immediately after
//! migrations finish.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde_json::json;
use time::OffsetDateTime;
use tracing::info;
use uuid::Uuid;

use super::entity::{
    enums::{QuotaPeriod, SystemSettingValueKind},
    host_policy, quota_limit, quota_plan, system_setting, team_group,
};

#[derive(Clone, Copy)]
struct QuotaPlanSeed {
    code: &'static str,
    name: &'static str,
    description: &'static str,
}

#[derive(Clone, Copy)]
struct TeamGroupSeed {
    code: &'static str,
    name: &'static str,
    description: &'static str,
    quota_plan_code: &'static str,
    host_policy: HostPolicySeed,
}

#[derive(Clone, Copy)]
struct HostPolicySeed {
    max_hosts: i32,
    allow_custom_hosts: bool,
    allow_auto_assign: bool,
}

#[derive(Clone)]
struct QuotaLimitSeed {
    dimension: &'static str,
    period: QuotaPeriod,
    free: i64,
    student: i64,
    plus: i64,
    pro: i64,
    ultra: i64,
}

impl QuotaLimitSeed {
    fn value_for(self, plan_code: &str) -> i64 {
        match plan_code {
            "free" => self.free,
            "student" => self.student,
            "plus" => self.plus,
            "pro" => self.pro,
            "ultra" => self.ultra,
            _ => self.free,
        }
    }
}

#[derive(Clone)]
struct SystemSettingSeed {
    key: &'static str,
    value_kind: SystemSettingValueKind,
    value_json: serde_json::Value,
}

const DEFAULT_QUOTA_PLAN_CODE: &str = "free";
const DEFAULT_TEAM_GROUP_CODE: &str = "free";

const DEFAULT_QUOTA_PLANS: &[QuotaPlanSeed] = &[
    QuotaPlanSeed {
        code: "free",
        name: "Free",
        description: "Default free plan for personal and small test projects.",
    },
    QuotaPlanSeed {
        code: "student",
        name: "Student",
        description: "Student plan with moderate deployment and host quotas.",
    },
    QuotaPlanSeed {
        code: "plus",
        name: "Plus",
        description: "Plus plan for growing teams and projects.",
    },
    QuotaPlanSeed {
        code: "pro",
        name: "Pro",
        description: "Professional plan for production workloads.",
    },
    QuotaPlanSeed {
        code: "ultra",
        name: "Ultra",
        description: "Highest first-stage quota plan for large teams.",
    },
];

const DEFAULT_TEAM_GROUPS: &[TeamGroupSeed] = &[
    TeamGroupSeed {
        code: "free",
        name: "Free",
        description: "Default team group for personal teams.",
        quota_plan_code: "free",
        host_policy: HostPolicySeed {
            max_hosts: 3,
            allow_custom_hosts: false,
            allow_auto_assign: true,
        },
    },
    TeamGroupSeed {
        code: "student",
        name: "Student",
        description: "Team group for student usage.",
        quota_plan_code: "student",
        host_policy: HostPolicySeed {
            max_hosts: 8,
            allow_custom_hosts: true,
            allow_auto_assign: true,
        },
    },
    TeamGroupSeed {
        code: "plus",
        name: "Plus",
        description: "Team group for plus teams.",
        quota_plan_code: "plus",
        host_policy: HostPolicySeed {
            max_hosts: 20,
            allow_custom_hosts: true,
            allow_auto_assign: true,
        },
    },
    TeamGroupSeed {
        code: "pro",
        name: "Pro",
        description: "Team group for professional teams.",
        quota_plan_code: "pro",
        host_policy: HostPolicySeed {
            max_hosts: 80,
            allow_custom_hosts: true,
            allow_auto_assign: true,
        },
    },
    TeamGroupSeed {
        code: "ultra",
        name: "Ultra",
        description: "Team group for highest first-stage limits.",
        quota_plan_code: "ultra",
        host_policy: HostPolicySeed {
            max_hosts: 300,
            allow_custom_hosts: true,
            allow_auto_assign: true,
        },
    },
];

const DEFAULT_QUOTA_LIMITS: &[QuotaLimitSeed] = &[
    QuotaLimitSeed {
        dimension: "projects",
        period: QuotaPeriod::None,
        free: 3,
        student: 8,
        plus: 20,
        pro: 80,
        ultra: 300,
    },
    QuotaLimitSeed {
        dimension: "deployments.monthly",
        period: QuotaPeriod::Monthly,
        free: 100,
        student: 500,
        plus: 2_000,
        pro: 10_000,
        ultra: 50_000,
    },
    QuotaLimitSeed {
        dimension: "build_minutes.monthly",
        period: QuotaPeriod::Monthly,
        free: 200,
        student: 1_000,
        plus: 5_000,
        pro: 25_000,
        ultra: 120_000,
    },
    QuotaLimitSeed {
        dimension: "hosts",
        period: QuotaPeriod::None,
        free: 3,
        student: 8,
        plus: 20,
        pro: 80,
        ultra: 300,
    },
    QuotaLimitSeed {
        dimension: "storage_mb",
        period: QuotaPeriod::None,
        free: 1_024,
        student: 5_120,
        plus: 20_480,
        pro: 102_400,
        ultra: 512_000,
    },
];

fn default_system_settings() -> Vec<SystemSettingSeed> {
    vec![
        SystemSettingSeed {
            key: "release_review_policy.default",
            value_kind: SystemSettingValueKind::Json,
            value_json: json!({
                "production": "manual",
                "preview": "auto",
            }),
        },
        SystemSettingSeed {
            key: "team_roles.default",
            value_kind: SystemSettingValueKind::Json,
            value_json: json!(["owner", "admin", "member", "viewer"]),
        },
    ]
}

pub async fn run(database: &DatabaseConnection) -> anyhow::Result<()> {
    seed_quota_plans(database).await?;
    seed_team_groups(database).await?;
    seed_quota_limits(database).await?;
    seed_host_policies(database).await?;
    seed_system_settings(database).await?;

    info!(operation = "control_api.seed", "database seed completed");

    Ok(())
}

async fn seed_quota_plans(database: &DatabaseConnection) -> anyhow::Result<()> {
    for plan in DEFAULT_QUOTA_PLANS {
        upsert_quota_plan(database, *plan).await?;
    }

    Ok(())
}

async fn upsert_quota_plan(
    database: &DatabaseConnection,
    seed: QuotaPlanSeed,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let is_default =
        seed.code == DEFAULT_QUOTA_PLAN_CODE && !quota_plan_default_exists(database).await?;

    let existing = quota_plan::Entity::find()
        .filter(quota_plan::Column::Code.eq(seed.code))
        .one(database)
        .await?;

    if existing.is_none() {
        quota_plan::ActiveModel {
            id: Set(Uuid::now_v7()),
            code: Set(seed.code.to_owned()),
            name: Set(seed.name.to_owned()),
            description: Set(Some(seed.description.to_owned())),
            is_default: Set(is_default),
            enabled: Set(true),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(database)
        .await?;
    }

    Ok(())
}

async fn quota_plan_default_exists(database: &DatabaseConnection) -> anyhow::Result<bool> {
    Ok(quota_plan::Entity::find()
        .filter(quota_plan::Column::IsDefault.eq(true))
        .one(database)
        .await?
        .is_some())
}

async fn seed_team_groups(database: &DatabaseConnection) -> anyhow::Result<()> {
    for group in DEFAULT_TEAM_GROUPS {
        upsert_team_group(database, *group).await?;
    }

    Ok(())
}

async fn upsert_team_group(
    database: &DatabaseConnection,
    seed: TeamGroupSeed,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let is_default =
        seed.code == DEFAULT_TEAM_GROUP_CODE && !team_group_default_exists(database).await?;

    let existing = team_group::Entity::find()
        .filter(team_group::Column::Code.eq(seed.code))
        .one(database)
        .await?;

    match existing {
        Some(model) => {
            if model.quota_plan_id.is_none() {
                let mut active: team_group::ActiveModel = model.into();
                active.quota_plan_id = Set(Some(
                    quota_plan_id_by_code(database, seed.quota_plan_code).await?,
                ));
                active.updated_at = Set(now);
                active.update(database).await?;
            }
        }
        None => {
            team_group::ActiveModel {
                id: Set(Uuid::now_v7()),
                code: Set(seed.code.to_owned()),
                name: Set(seed.name.to_owned()),
                description: Set(Some(seed.description.to_owned())),
                quota_plan_id: Set(Some(
                    quota_plan_id_by_code(database, seed.quota_plan_code).await?,
                )),
                is_default: Set(is_default),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(database)
            .await?;
        }
    }

    Ok(())
}

async fn team_group_default_exists(database: &DatabaseConnection) -> anyhow::Result<bool> {
    Ok(team_group::Entity::find()
        .filter(team_group::Column::IsDefault.eq(true))
        .one(database)
        .await?
        .is_some())
}

async fn seed_quota_limits(database: &DatabaseConnection) -> anyhow::Result<()> {
    for plan in DEFAULT_QUOTA_PLANS {
        let quota_plan_id = quota_plan_id_by_code(database, plan.code).await?;

        for limit in DEFAULT_QUOTA_LIMITS {
            upsert_quota_limit(database, quota_plan_id, plan.code, limit.clone()).await?;
        }
    }

    Ok(())
}

async fn upsert_quota_limit(
    database: &DatabaseConnection,
    quota_plan_id: Uuid,
    quota_plan_code: &str,
    seed: QuotaLimitSeed,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let period = seed.period.clone();
    let existing = quota_limit::Entity::find()
        .filter(quota_limit::Column::QuotaPlanId.eq(quota_plan_id))
        .filter(quota_limit::Column::Dimension.eq(seed.dimension))
        .filter(quota_limit::Column::Period.eq(period.clone()))
        .one(database)
        .await?;

    if existing.is_none() {
        quota_limit::ActiveModel {
            id: Set(Uuid::now_v7()),
            quota_plan_id: Set(quota_plan_id),
            dimension: Set(seed.dimension.to_owned()),
            limit_value: Set(seed.value_for(quota_plan_code)),
            period: Set(period),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(database)
        .await?;
    }

    Ok(())
}

async fn seed_host_policies(database: &DatabaseConnection) -> anyhow::Result<()> {
    for group in DEFAULT_TEAM_GROUPS {
        upsert_host_policy(database, *group).await?;
    }

    Ok(())
}

async fn upsert_host_policy(
    database: &DatabaseConnection,
    group: TeamGroupSeed,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let team_group_id = team_group_id_by_code(database, group.code).await?;
    let quota_plan_id = quota_plan_id_by_code(database, group.quota_plan_code).await?;
    let policy = group.host_policy;

    let existing = host_policy::Entity::find()
        .filter(host_policy::Column::TeamGroupId.eq(team_group_id))
        .one(database)
        .await?;

    match existing {
        Some(model) => {
            if model.quota_plan_id.is_none() {
                let mut active: host_policy::ActiveModel = model.into();
                active.quota_plan_id = Set(Some(quota_plan_id));
                active.updated_at = Set(now);
                active.update(database).await?;
            }
        }
        None => {
            host_policy::ActiveModel {
                id: Set(Uuid::now_v7()),
                team_group_id: Set(Some(team_group_id)),
                quota_plan_id: Set(Some(quota_plan_id)),
                max_hosts: Set(policy.max_hosts),
                allow_custom_hosts: Set(policy.allow_custom_hosts),
                allow_auto_assign: Set(policy.allow_auto_assign),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(database)
            .await?;
        }
    }

    Ok(())
}

async fn seed_system_settings(database: &DatabaseConnection) -> anyhow::Result<()> {
    for setting in default_system_settings() {
        upsert_system_setting(database, setting).await?;
    }

    Ok(())
}

async fn upsert_system_setting(
    database: &DatabaseConnection,
    seed: SystemSettingSeed,
) -> anyhow::Result<()> {
    let now = OffsetDateTime::now_utc();
    let existing = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq(seed.key))
        .one(database)
        .await?;

    if existing.is_none() {
        system_setting::ActiveModel {
            id: Set(Uuid::now_v7()),
            key: Set(seed.key.to_owned()),
            value_kind: Set(seed.value_kind),
            value: Set(seed.value_json),
            is_secret: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(database)
        .await?;
    }

    Ok(())
}

async fn quota_plan_id_by_code(database: &DatabaseConnection, code: &str) -> anyhow::Result<Uuid> {
    quota_plan::Entity::find()
        .filter(quota_plan::Column::Code.eq(code))
        .one(database)
        .await?
        .map(|plan| plan.id)
        .ok_or_else(|| anyhow::anyhow!("default quota plan {code} was not seeded"))
}

async fn team_group_id_by_code(database: &DatabaseConnection, code: &str) -> anyhow::Result<Uuid> {
    team_group::Entity::find()
        .filter(team_group::Column::Code.eq(code))
        .one(database)
        .await?
        .map(|group| group.id)
        .ok_or_else(|| anyhow::anyhow!("default team group {code} was not seeded"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn default_seed_codes_are_unique() {
        assert_unique(DEFAULT_QUOTA_PLANS.iter().map(|plan| plan.code));
        assert_unique(DEFAULT_TEAM_GROUPS.iter().map(|group| group.code));
        assert_unique(default_system_settings().iter().map(|setting| setting.key));
    }

    #[test]
    fn roadmap_required_team_groups_are_seeded() {
        let codes = DEFAULT_TEAM_GROUPS
            .iter()
            .map(|group| group.code)
            .collect::<HashSet<_>>();

        for required in ["free", "student", "plus", "pro", "ultra"] {
            assert!(
                codes.contains(required),
                "missing default team group {required}"
            );
        }
    }

    #[test]
    fn default_groups_reference_existing_quota_plans() {
        let plan_codes = DEFAULT_QUOTA_PLANS
            .iter()
            .map(|plan| plan.code)
            .collect::<HashSet<_>>();

        for group in DEFAULT_TEAM_GROUPS {
            assert!(
                plan_codes.contains(group.quota_plan_code),
                "group {} references missing quota plan {}",
                group.code,
                group.quota_plan_code
            );
        }
    }

    #[test]
    fn default_settings_are_json_values() {
        for setting in default_system_settings() {
            assert!(
                matches!(setting.value_kind, SystemSettingValueKind::Json),
                "setting {} should be a json setting",
                setting.key
            );
        }
    }

    fn assert_unique<'a>(values: impl Iterator<Item = &'a str>) {
        let mut seen = HashSet::new();
        for value in values {
            assert!(seen.insert(value), "duplicate seed value {value}");
        }
    }
}
