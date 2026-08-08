//! Platform-admin cleanup operations for persisted logs.

use std::collections::{HashMap, HashSet};

use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::retention::deployment_is_protected,
    infra::{
        database::entity::{
            DeploymentArtifactKind, deployment, deployment_artifact, node_deployment_migration,
        },
        storage::StorageManager,
    },
};

#[derive(Debug, Default, Clone)]
pub struct BuildLogFilter {
    pub deployment_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub triggered_by_user_id: Option<Uuid>,
    pub created_from: Option<OffsetDateTime>,
    pub created_to: Option<OffsetDateTime>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildLogCleanupSummary {
    pub matched: u64,
    pub deletable: u64,
    pub skipped: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildLogDeleteResult {
    pub deleted: u64,
    pub failed: u64,
    pub skipped: u64,
}

async fn candidates<C: ConnectionTrait>(
    db: &C,
    filter: &BuildLogFilter,
) -> anyhow::Result<Vec<(deployment_artifact::Model, deployment::Model, bool)>> {
    let mut deployments = deployment::Entity::find();
    if let Some(deployment_id) = filter.deployment_id {
        deployments = deployments.filter(deployment::Column::Id.eq(deployment_id));
    }
    if let Some(project_id) = filter.project_id {
        deployments = deployments.filter(deployment::Column::ProjectId.eq(project_id));
    }
    if let Some(team_id) = filter.team_id {
        deployments = deployments.filter(deployment::Column::TeamId.eq(team_id));
    }
    if let Some(user_id) = filter.triggered_by_user_id {
        deployments = deployments.filter(deployment::Column::TriggeredByUserId.eq(user_id));
    }
    let deployments = deployments.all(db).await?;
    if deployments.is_empty() {
        return Ok(Vec::new());
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
        .into_iter()
        .map(|item| (item.id, item))
        .collect::<HashMap<_, _>>();

    let mut artifacts = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.is_in(deployment_ids))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::BuildLog))
        .filter(deployment_artifact::Column::DeletedAt.is_null());
    if let Some(created_from) = filter.created_from {
        artifacts = artifacts.filter(deployment_artifact::Column::CreatedAt.gte(created_from));
    }
    if let Some(created_to) = filter.created_to {
        artifacts = artifacts.filter(deployment_artifact::Column::CreatedAt.lte(created_to));
    }

    Ok(artifacts
        .all(db)
        .await?
        .into_iter()
        .filter_map(|artifact| {
            let deployment = deployment_map.get(&artifact.deployment_id)?.clone();
            let protected =
                deployment_is_protected(&deployment, migrating.contains(&deployment.id));
            Some((artifact, deployment, protected))
        })
        .collect())
}

pub async fn summarize_build_logs<C: ConnectionTrait>(
    db: &C,
    filter: &BuildLogFilter,
) -> anyhow::Result<BuildLogCleanupSummary> {
    let candidates = candidates(db, filter).await?;
    let matched = candidates.len() as u64;
    let skipped = candidates
        .iter()
        .filter(|(_, _, protected)| *protected)
        .count() as u64;
    Ok(BuildLogCleanupSummary {
        matched,
        deletable: matched.saturating_sub(skipped),
        skipped,
    })
}

pub async fn delete_build_logs<C: ConnectionTrait>(
    db: &C,
    storage: &StorageManager,
    filter: &BuildLogFilter,
) -> anyhow::Result<BuildLogDeleteResult> {
    let candidates = candidates(db, filter).await?;
    let skipped = candidates
        .iter()
        .filter(|(_, _, protected)| *protected)
        .count() as u64;
    let mut result = BuildLogDeleteResult {
        skipped,
        ..Default::default()
    };
    let now = OffsetDateTime::now_utc();

    for (artifact, _, protected) in candidates {
        if protected {
            continue;
        }
        let mut marked = artifact.clone();
        marked.deleted_at = Some(now);
        let active: deployment_artifact::ActiveModel = marked.into();
        active.update(db).await?;
        match storage.remove(&artifact.storage_path).await {
            Ok(()) => result.deleted += 1,
            Err(error) => {
                result.failed += 1;
                tracing::warn!(
                    operation = "admin.cleanup.build_log",
                    %error,
                    artifact_id = %artifact.id,
                    "failed to remove build log file after tombstoning"
                );
            }
        }
    }

    Ok(result)
}
