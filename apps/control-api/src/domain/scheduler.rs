use std::{cmp::Ordering, collections::HashMap};

use grass_node_protocol::{NodeResources, ServeResources};
use rand::{Rng, seq::SliceRandom};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseTransaction, DbErr, FromQueryResult, Statement,
};
use uuid::Uuid;

const PLACEMENT_LOCK_ID: i32 = 1_196_575_564;

const NODE_USAGE_SQL: &str = r#"
SELECT
    d.serve_node_id AS node_id,
    COALESCE(SUM(d.serve_cpu_millicores), 0)::BIGINT AS used_cpu_millicores,
    COALESCE(SUM(d.serve_memory_mb), 0)::BIGINT AS used_memory_mb,
    COALESCE(SUM(d.serve_disk_mb), 0)::BIGINT AS used_disk_mb,
    COUNT(d.id)::BIGINT AS used_deployments
FROM deployments d
WHERE d.serve_node_id IS NOT NULL
    AND d.deleted_at IS NULL
    AND d.build_status NOT IN ('failed', 'canceled')
    AND d.serve_status <> 'retired'
GROUP BY d.serve_node_id
"#;

const ELIGIBLE_CANDIDATES_SQL: &str = r#"
SELECT
    n.id AS node_id,
    n.capacity_cpu_millicores,
    n.capacity_memory_mb,
    n.capacity_disk_mb,
    n.max_deployments,
    COALESCE(SUM(d.serve_cpu_millicores), 0)::BIGINT AS used_cpu_millicores,
    COALESCE(SUM(d.serve_memory_mb), 0)::BIGINT AS used_memory_mb,
    COALESCE(SUM(d.serve_disk_mb), 0)::BIGINT AS used_disk_mb,
    COUNT(d.id)::BIGINT AS used_deployments
FROM nodes n
LEFT JOIN deployments d
    ON d.serve_node_id = n.id
    AND d.deleted_at IS NULL
    AND d.build_status NOT IN ('failed', 'canceled')
    AND d.serve_status <> 'retired'
WHERE n.deleted_at IS NULL
    AND n.status = 'active'
    AND n.serve_enabled = TRUE
    AND n.last_heartbeat_at >= NOW() - INTERVAL '90 seconds'
    AND NULLIF(BTRIM(n.base_url), '') IS NOT NULL
    AND n.capacity_cpu_millicores > 0
    AND n.capacity_memory_mb > 0
    AND n.capacity_disk_mb > 0
    AND n.max_deployments > 0
GROUP BY n.id
ORDER BY n.id
"#;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct NodeUsage {
    pub cpu_millicores: u64,
    pub memory_mb: u64,
    pub disk_mb: u64,
    pub deployments: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub node_id: Uuid,
    pub capacity: NodeResources,
    pub usage: NodeUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementMode {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub node_id: Uuid,
    pub overcommitted: bool,
    pub mode: PlacementMode,
}

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("no serve node has enough capacity")]
    NoCapacity,
    #[error("selected serve node is unavailable")]
    SelectedNodeUnavailable,
    #[error("selected serve node has no remaining capacity")]
    SelectedNodeCapacity,
    #[error("serve node resource data is invalid")]
    InvalidData,
    #[error(transparent)]
    Database(#[from] DbErr),
}

#[derive(Debug, Clone, Copy)]
struct ProjectedUsage {
    cpu_ratio: f64,
    memory_ratio: f64,
    disk_ratio: f64,
    deployment_ratio: f64,
    disk_mb: u64,
    deployments: u64,
}

impl ProjectedUsage {
    fn all_at_most_one(self) -> bool {
        self.cpu_ratio <= 1.0
            && self.memory_ratio <= 1.0
            && self.disk_ratio <= 1.0
            && self.deployment_ratio <= 1.0
    }

    fn dominant_ratio(self) -> f64 {
        self.cpu_ratio
            .max(self.memory_ratio)
            .max(self.disk_ratio)
            .max(self.deployment_ratio)
    }
}

impl Candidate {
    fn has_positive_capacity(&self) -> bool {
        self.capacity.cpu_millicores > 0
            && self.capacity.memory_mb > 0
            && self.capacity.disk_mb > 0
            && self.capacity.max_deployments > 0
    }

    fn projected(&self, requested: ServeResources) -> ProjectedUsage {
        let cpu = self
            .usage
            .cpu_millicores
            .saturating_add(requested.cpu_millicores);
        let memory = self.usage.memory_mb.saturating_add(requested.memory_mb);
        let disk = self.usage.disk_mb.saturating_add(requested.disk_mb);
        let deployments = self.usage.deployments.saturating_add(1);
        ProjectedUsage {
            cpu_ratio: ratio(cpu, self.capacity.cpu_millicores),
            memory_ratio: ratio(memory, self.capacity.memory_mb),
            disk_ratio: ratio(disk, self.capacity.disk_mb),
            deployment_ratio: ratio(deployments, u64::from(self.capacity.max_deployments)),
            disk_mb: disk,
            deployments,
        }
    }

    fn can_overflow(&self, requested: ServeResources) -> bool {
        let projected = self.projected(requested);
        projected.disk_mb <= self.capacity.disk_mb
            && projected.deployments <= u64::from(self.capacity.max_deployments).saturating_add(2)
    }
}

fn ratio(used: u64, capacity: u64) -> f64 {
    if capacity == 0 {
        f64::INFINITY
    } else {
        used as f64 / capacity as f64
    }
}

pub fn choose_candidate(
    candidates: &[Candidate],
    requested: ServeResources,
    selected_node_id: Option<Uuid>,
) -> Result<Placement, ScheduleError> {
    choose_candidate_with_rng(
        candidates,
        requested,
        selected_node_id,
        &mut rand::thread_rng(),
    )
}

pub fn choose_candidate_with_rng<R: Rng + ?Sized>(
    candidates: &[Candidate],
    requested: ServeResources,
    selected_node_id: Option<Uuid>,
    rng: &mut R,
) -> Result<Placement, ScheduleError> {
    if let Some(selected_node_id) = selected_node_id {
        let candidate = candidates
            .iter()
            .find(|candidate| {
                candidate.node_id == selected_node_id && candidate.has_positive_capacity()
            })
            .ok_or(ScheduleError::SelectedNodeUnavailable)?;
        if !candidate.can_overflow(requested) {
            return Err(ScheduleError::SelectedNodeCapacity);
        }
        return Ok(Placement {
            node_id: candidate.node_id,
            overcommitted: !candidate.projected(requested).all_at_most_one(),
            mode: PlacementMode::Manual,
        });
    }

    let eligible = candidates
        .iter()
        .filter(|candidate| candidate.has_positive_capacity());
    if let Some(candidate) = choose_lowest_random(
        eligible
            .clone()
            .filter(|candidate| candidate.projected(requested).all_at_most_one()),
        requested,
        rng,
    ) {
        return Ok(Placement {
            node_id: candidate.node_id,
            overcommitted: false,
            mode: PlacementMode::Automatic,
        });
    }

    choose_lowest_random(
        eligible.filter(|candidate| candidate.can_overflow(requested)),
        requested,
        rng,
    )
    .map(|candidate| Placement {
        node_id: candidate.node_id,
        overcommitted: true,
        mode: PlacementMode::Automatic,
    })
    .ok_or(ScheduleError::NoCapacity)
}

fn choose_lowest_random<'a, I, R>(
    candidates: I,
    requested: ServeResources,
    rng: &mut R,
) -> Option<&'a Candidate>
where
    I: Iterator<Item = &'a Candidate>,
    R: Rng + ?Sized,
{
    let mut minimum = None;
    let mut tied = Vec::new();
    for candidate in candidates {
        let score = candidate.projected(requested).dominant_ratio();
        match minimum.map(|value: f64| score.total_cmp(&value)) {
            None | Some(Ordering::Less) => {
                minimum = Some(score);
                tied.clear();
                tied.push(candidate);
            }
            Some(Ordering::Equal) => tied.push(candidate),
            Some(Ordering::Greater) => {}
        }
    }
    tied.choose(rng).copied()
}

pub async fn place_deployment(
    transaction: &DatabaseTransaction,
    requested: ServeResources,
    selected_node_id: Option<Uuid>,
) -> Result<Placement, ScheduleError> {
    lock_placement(transaction).await?;
    let candidates = eligible_candidates(transaction).await?;
    choose_candidate(&candidates, requested, selected_node_id)
}

/// Serializes operations that can change whether a Serve Node has room for
/// another deployment, including placement and administrative capacity edits.
pub async fn lock_placement(transaction: &DatabaseTransaction) -> Result<(), ScheduleError> {
    transaction
        .execute_unprepared(&format!(
            "SELECT pg_advisory_xact_lock({PLACEMENT_LOCK_ID})"
        ))
        .await?;
    Ok(())
}

pub async fn node_usage<C: ConnectionTrait>(
    db: &C,
) -> Result<HashMap<Uuid, NodeUsage>, ScheduleError> {
    let rows = NodeUsageRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Postgres,
        NODE_USAGE_SQL,
    ))
    .all(db)
    .await?;

    rows.into_iter()
        .map(|row| {
            let node_id = row.node_id;
            Ok((node_id, row.try_into()?))
        })
        .collect()
}

pub async fn eligible_candidates<C: ConnectionTrait>(
    db: &C,
) -> Result<Vec<Candidate>, ScheduleError> {
    let rows = CandidateRow::find_by_statement(Statement::from_string(
        DatabaseBackend::Postgres,
        ELIGIBLE_CANDIDATES_SQL,
    ))
    .all(db)
    .await?;

    rows.into_iter().map(Candidate::try_from).collect()
}

#[derive(Debug, FromQueryResult)]
struct CandidateRow {
    node_id: Uuid,
    capacity_cpu_millicores: i64,
    capacity_memory_mb: i64,
    capacity_disk_mb: i64,
    max_deployments: i32,
    used_cpu_millicores: i64,
    used_memory_mb: i64,
    used_disk_mb: i64,
    used_deployments: i64,
}

#[derive(Debug, FromQueryResult)]
struct NodeUsageRow {
    node_id: Uuid,
    used_cpu_millicores: i64,
    used_memory_mb: i64,
    used_disk_mb: i64,
    used_deployments: i64,
}

impl TryFrom<NodeUsageRow> for NodeUsage {
    type Error = ScheduleError;

    fn try_from(row: NodeUsageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            cpu_millicores: row
                .used_cpu_millicores
                .try_into()
                .map_err(|_| ScheduleError::InvalidData)?,
            memory_mb: row
                .used_memory_mb
                .try_into()
                .map_err(|_| ScheduleError::InvalidData)?,
            disk_mb: row
                .used_disk_mb
                .try_into()
                .map_err(|_| ScheduleError::InvalidData)?,
            deployments: row
                .used_deployments
                .try_into()
                .map_err(|_| ScheduleError::InvalidData)?,
        })
    }
}

impl TryFrom<CandidateRow> for Candidate {
    type Error = ScheduleError;

    fn try_from(row: CandidateRow) -> Result<Self, Self::Error> {
        Ok(Self {
            node_id: row.node_id,
            capacity: NodeResources {
                cpu_millicores: row
                    .capacity_cpu_millicores
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                memory_mb: row
                    .capacity_memory_mb
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                disk_mb: row
                    .capacity_disk_mb
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                max_deployments: row
                    .max_deployments
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
            },
            usage: NodeUsage {
                cpu_millicores: row
                    .used_cpu_millicores
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                memory_mb: row
                    .used_memory_mb
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                disk_mb: row
                    .used_disk_mb
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
                deployments: row
                    .used_deployments
                    .try_into()
                    .map_err(|_| ScheduleError::InvalidData)?,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use grass_node_protocol::{NodeResources, ServeResources};
    use rand::{SeedableRng, rngs::StdRng};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn capacity_queries_ignore_retired_deployments() {
        assert!(NODE_USAGE_SQL.contains("d.serve_status <> 'retired'"));
        assert!(ELIGIBLE_CANDIDATES_SQL.contains("d.serve_status <> 'retired'"));
    }

    fn candidate(
        id: u128,
        capacity: (u64, u64, u64, u32),
        usage: (u64, u64, u64, u64),
    ) -> Candidate {
        Candidate {
            node_id: Uuid::from_u128(id),
            capacity: NodeResources {
                cpu_millicores: capacity.0,
                memory_mb: capacity.1,
                disk_mb: capacity.2,
                max_deployments: capacity.3,
            },
            usage: NodeUsage {
                cpu_millicores: usage.0,
                memory_mb: usage.1,
                disk_mb: usage.2,
                deployments: usage.3,
            },
        }
    }

    fn request() -> ServeResources {
        ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        }
    }

    #[test]
    fn normal_capacity_wins_before_any_overflow() {
        let full = candidate(1, (2_000, 2_000, 10_000, 10), (2_000, 1_000, 1_000, 10));
        let normal = candidate(2, (2_000, 2_000, 10_000, 10), (1_500, 1_500, 2_000, 8));

        let placement = choose_candidate(&[full, normal.clone()], request(), None).unwrap();

        assert_eq!(placement.node_id, normal.node_id);
        assert!(!placement.overcommitted);
        assert_eq!(placement.mode, PlacementMode::Automatic);
    }

    #[test]
    fn selection_uses_projected_dominant_resource() {
        let lower_current = candidate(1, (1_000, 1_000, 10_000, 10), (0, 0, 0, 0));
        let lower_projected = candidate(2, (10_000, 10_000, 10_000, 10), (1_000, 1_000, 0, 0));

        let placement = choose_candidate(
            &[lower_current, lower_projected.clone()],
            ServeResources {
                cpu_millicores: 500,
                memory_mb: 500,
                disk_mb: 100,
            },
            None,
        )
        .unwrap();

        assert_eq!(placement.node_id, lower_projected.node_id);
    }

    #[test]
    fn exact_ties_choose_only_from_the_minimum_set_and_randomize() {
        let first = candidate(1, (2_000, 2_000, 10_000, 10), (500, 500, 500, 2));
        let second = candidate(2, (2_000, 2_000, 10_000, 10), (500, 500, 500, 2));
        let worse = candidate(3, (2_000, 2_000, 10_000, 10), (1_500, 1_500, 500, 2));
        let mut selected = HashSet::new();

        for seed in 0..32 {
            let mut rng = StdRng::seed_from_u64(seed);
            selected.insert(
                choose_candidate_with_rng(
                    &[first.clone(), second.clone(), worse.clone()],
                    request(),
                    None,
                    &mut rng,
                )
                .unwrap()
                .node_id,
            );
        }

        assert_eq!(selected, HashSet::from([first.node_id, second.node_id]));
    }

    #[test]
    fn overflow_never_crosses_disk_or_two_extra_slots() {
        let disk_full = candidate(1, (2_000, 2_000, 10_000, 10), (2_000, 2_000, 9_900, 11));
        let slots_full = candidate(2, (2_000, 2_000, 10_000, 10), (2_000, 2_000, 1_000, 12));

        assert!(choose_candidate(&[disk_full], request(), None).is_err());
        assert!(choose_candidate(&[slots_full], request(), None).is_err());
    }

    #[test]
    fn manual_selection_may_use_a_valid_overflow_slot() {
        let selected = candidate(1, (2_000, 2_000, 10_000, 10), (2_000, 2_000, 1_000, 10));
        let normal = candidate(2, (2_000, 2_000, 10_000, 10), (0, 0, 0, 0));

        let placement = choose_candidate(
            &[selected.clone(), normal],
            request(),
            Some(selected.node_id),
        )
        .unwrap();

        assert_eq!(placement.node_id, selected.node_id);
        assert!(placement.overcommitted);
        assert_eq!(placement.mode, PlacementMode::Manual);
    }
}
