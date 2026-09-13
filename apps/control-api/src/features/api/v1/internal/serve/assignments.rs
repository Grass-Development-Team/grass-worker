use axum::{Extension, extract::State, response::IntoResponse};
use grass_node_protocol::{ServeArtifact, ServeAssignment, ServeAssignmentStatus, ServeResources};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    domain::{deployments, node_migration_access::shadow_migration_statuses, projects},
    infra::{
        database::entity::{
            DeploymentArtifactKind, DeploymentBuildStatus, DeploymentServeStatus,
            NodeDeploymentMigrationStatus, deployment, deployment_artifact,
            node_deployment_migration,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServeAssignmentsResponse {
    assignments: Vec<ServeAssignment>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/assignments", axum::routing::get(assignments))
}

fn ensure_serve_node(
    node: &crate::infra::database::entity::node::Model,
    op: &'static str,
) -> Result<(), AppError> {
    if !node.serve_enabled {
        return Err(AppError::Forbidden {
            op,
            message: "node does not have Serve capability".to_owned(),
        });
    }
    Ok(())
}

/// GET /api/v1/internal/serve/assignments
async fn assignments(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.assignments";
    ensure_serve_node(&node, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let assigned = deployment::Entity::find()
        .filter(deployment::Column::ServeNodeId.eq(node.id))
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
        .filter(deployment::Column::ServeStatus.ne(DeploymentServeStatus::Retired))
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_asc(deployment::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let migrations = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::TargetNodeId.eq(node.id))
        .filter(node_deployment_migration::Column::Status.is_in(shadow_migration_statuses()))
        .order_by_asc(node_deployment_migration::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let migration_deployment_ids = migrations
        .iter()
        .map(|migration| migration.deployment_id)
        .collect::<Vec<_>>();
    let migration_deployments = if migration_deployment_ids.is_empty() {
        Vec::new()
    } else {
        deployment::Entity::find()
            .filter(deployment::Column::Id.is_in(migration_deployment_ids))
            .filter(
                Condition::any()
                    .add(deployment::Column::ServeNodeId.ne(node.id))
                    .add(deployment::Column::ServeNodeId.is_null()),
            )
            .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
            .filter(deployment::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
    };
    if assigned.is_empty() && migration_deployments.is_empty() {
        return Ok(ok_response(ServeAssignmentsResponse {
            assignments: Vec::new(),
        }));
    }

    let deployment_ids = assigned
        .iter()
        .chain(migration_deployments.iter())
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let artifacts = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.is_in(deployment_ids))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
        .filter(deployment_artifact::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let artifacts = artifacts
        .into_iter()
        .map(|artifact| (artifact.deployment_id, artifact))
        .collect::<HashMap<_, _>>();

    let migration_statuses = migrations
        .into_iter()
        .map(|migration| (migration.deployment_id, migration.status))
        .collect::<HashMap<_, _>>();
    let mut assignments = Vec::with_capacity(assigned.len() + migration_deployments.len());
    for (item, migration_status) in
        assigned
            .into_iter()
            .map(|item| (item, None))
            .chain(migration_deployments.into_iter().map(|item| {
                let status = migration_statuses.get(&item.id).cloned();
                (item, status)
            }))
    {
        let Some(artifact) = artifacts.get(&item.id) else {
            continue;
        };
        let metadata_error = || AppError::Internal {
            op: OP,
            message: "assigned deployment has invalid artifact metadata".to_owned(),
        };
        assignments.push(ServeAssignment {
            deployment_id: item.id,
            project_id: item.project_id,
            runtime_kind: projects::runtime_value(&item.runtime_kind).to_owned(),
            status: match migration_status {
                Some(NodeDeploymentMigrationStatus::Pending) => ServeAssignmentStatus::Pending,
                Some(NodeDeploymentMigrationStatus::Syncing) => ServeAssignmentStatus::Syncing,
                Some(NodeDeploymentMigrationStatus::Failed) => ServeAssignmentStatus::Failed,
                Some(NodeDeploymentMigrationStatus::Ready) => ServeAssignmentStatus::Ready,
                None => match item.serve_status {
                    DeploymentServeStatus::Pending => ServeAssignmentStatus::Pending,
                    DeploymentServeStatus::Syncing => ServeAssignmentStatus::Syncing,
                    DeploymentServeStatus::Ready => ServeAssignmentStatus::Ready,
                    DeploymentServeStatus::Failed => ServeAssignmentStatus::Failed,
                    DeploymentServeStatus::Retired => continue,
                },
            },
            artifact: ServeArtifact {
                artifact_id: artifact.id,
                checksum_sha256: artifact
                    .checksum_sha256
                    .clone()
                    .ok_or_else(metadata_error)?,
                packed_size_bytes: artifact
                    .size_bytes
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or_else(metadata_error)?,
                unpacked_size_bytes: deployments::artifact_unpacked_size_bytes(&artifact.manifest)
                    .ok_or_else(metadata_error)?,
            },
            resources: ServeResources {
                cpu_millicores: u64::try_from(item.serve_cpu_millicores)
                    .map_err(|_| metadata_error())?,
                memory_mb: u64::try_from(item.serve_memory_mb).map_err(|_| metadata_error())?,
                disk_mb: u64::try_from(item.serve_disk_mb).map_err(|_| metadata_error())?,
            },
        });
    }

    Ok(ok_response(ServeAssignmentsResponse { assignments }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_artifact_metadata_uses_unknown_unpacked_size() {
        let legacy = serde_json::json!({
            "runtime_kind": "static",
            "output_api_version": "1",
        });

        assert_eq!(
            deployments::artifact_unpacked_size_bytes(&legacy).unwrap(),
            0
        );
        assert_eq!(
            deployments::artifact_unpacked_size_bytes(&serde_json::json!({
                "unpacked_size_bytes": 12_345,
            }))
            .unwrap(),
            12_345
        );
        assert!(
            deployments::artifact_unpacked_size_bytes(&serde_json::json!({
                "unpacked_size_bytes": "invalid",
            }))
            .is_none()
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            ServeAssignmentsResponse,
            grass_node_protocol::ServeAssignmentsResponse,
        >("ServeAssignmentsResponse");
    }
}
