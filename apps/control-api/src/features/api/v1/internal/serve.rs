use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use grass_node_protocol::{
    ReportServeStatusRequest, ReportServeStatusResponse, ReportedServeStatus, ResolveHostResponse,
    RouteSnapshotResponse, ServeArtifact, ServeAssignment, ServeAssignmentStatus,
    ServeAssignmentsResponse, ServeResources, ServeRoute,
};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{deployments, hosts, projects},
    infra::{
        database::entity::{
            DeploymentArtifactKind, DeploymentBuildStatus, DeploymentEnvironment,
            DeploymentReleaseStatus, DeploymentServeStatus, HostBindingEnvironment,
            HostBindingStatus, NodeStatus, deployment, deployment_artifact, node,
            project_host_binding,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct ResolveHostQuery {
    pub host: String,
}

fn validate_status_report(report: &ReportServeStatusRequest) -> Result<(), &'static str> {
    if let Some(message) = &report.failure_message
        && message.len() > 1024
    {
        return Err("failure_message must be at most 1024 bytes");
    }
    if !matches!(report.status, ReportedServeStatus::Failed) {
        if report.failure_code.is_some() || report.failure_message.is_some() {
            return Err("failure details are only allowed for failed status");
        }
        return Ok(());
    }

    let Some(code) = report.failure_code.as_deref() else {
        return Err("failed status requires failure_code");
    };
    if code.is_empty()
        || code.len() > 64
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(&byte))
    {
        return Err("failure_code must be a lowercase identifier of at most 64 bytes");
    }
    Ok(())
}

fn route_revision(routes: &[ServeRoute]) -> String {
    let mut canonical = routes.iter().collect::<Vec<_>>();
    canonical.sort_by(|left, right| {
        (
            &left.host,
            left.deployment_id,
            left.target_node_id,
            &left.target_base_url,
        )
            .cmp(&(
                &right.host,
                right.deployment_id,
                right.target_node_id,
                &right.target_base_url,
            ))
    });
    hex::encode(Sha256::digest(
        serde_json::to_vec(&canonical).expect("Serve routes always serialize"),
    ))
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
pub async fn assignments(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.assignments";
    ensure_serve_node(&node, OP)?;
    let db = super::database(&state, OP)?;
    let assigned = deployment::Entity::find()
        .filter(deployment::Column::ServeNodeId.eq(node.id))
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_asc(deployment::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if assigned.is_empty() {
        return Ok(ok_response(ServeAssignmentsResponse {
            assignments: Vec::new(),
        }));
    }

    let deployment_ids = assigned.iter().map(|item| item.id).collect::<Vec<_>>();
    let artifacts = deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.is_in(deployment_ids))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
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

    let mut assignments = Vec::with_capacity(assigned.len());
    for item in assigned {
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
            status: match item.serve_status {
                DeploymentServeStatus::Pending => ServeAssignmentStatus::Pending,
                DeploymentServeStatus::Syncing => ServeAssignmentStatus::Syncing,
                DeploymentServeStatus::Ready => ServeAssignmentStatus::Ready,
                DeploymentServeStatus::Failed => ServeAssignmentStatus::Failed,
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
                unpacked_size_bytes: artifact
                    .manifest
                    .get("unpacked_size_bytes")
                    .and_then(serde_json::Value::as_u64)
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

/// POST /api/v1/internal/serve/deployments/{deployment_id}/status
pub async fn report_status(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
    Json(report): Json<ReportServeStatusRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.report_status";
    ensure_serve_node(&node, OP)?;
    validate_status_report(&report).map_err(|message| AppError::Validation {
        op: OP,
        message: message.to_owned(),
    })?;
    let db = super::database(&state, OP)?;
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.serve_node_id != Some(node.id) {
        return Err(AppError::Forbidden {
            op: OP,
            message: "deployment is not assigned to this Serve Node".to_owned(),
        });
    }
    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::Conflict {
            op: OP,
            message: "deployment build is not ready".to_owned(),
        });
    }

    let target = match report.status {
        ReportedServeStatus::Syncing => DeploymentServeStatus::Syncing,
        ReportedServeStatus::Ready => DeploymentServeStatus::Ready,
        ReportedServeStatus::Failed => DeploymentServeStatus::Failed,
    };
    let updated = deployments::transition_serve(
        db,
        deployment,
        deployments::ServeTransition {
            to: target.clone(),
            failure_code: report.failure_code,
            failure_message: report.failure_message,
        },
    )
    .await
    .map_err(|error| crate::features::api::v1::projects::deployments::map_state_error(error, OP))?;
    if matches!(target, DeploymentServeStatus::Ready) {
        super::deployments::auto_activate_if_allowed(db, updated).await?;
    }

    Ok(ok_response(ReportServeStatusResponse {
        acknowledged: true,
    }))
}

/// GET /api/v1/internal/serve/routes
pub async fn routes(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(requesting_node)): Extension<AuthenticatedNode>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.routes";
    ensure_serve_node(&requesting_node, OP)?;
    let db = super::database(&state, OP)?;
    let deployments = deployment::Entity::find()
        .filter(deployment::Column::BuildStatus.eq(DeploymentBuildStatus::Ready))
        .filter(deployment::Column::ServeStatus.eq(DeploymentServeStatus::Ready))
        .filter(deployment::Column::DeletedAt.is_null())
        .filter(
            Condition::any()
                .add(deployment::Column::PreviewHost.is_not_null())
                .add(deployment::Column::ReleaseStatus.eq(DeploymentReleaseStatus::Active)),
        )
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if deployments.is_empty() {
        return Ok(ok_response(RouteSnapshotResponse {
            revision: route_revision(&[]),
            routes: Vec::new(),
        }));
    }

    let node_ids = deployments
        .iter()
        .filter_map(|deployment| deployment.serve_node_id)
        .collect::<Vec<_>>();
    let nodes = node::Entity::find()
        .filter(node::Column::Id.is_in(node_ids))
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::Status.ne(NodeStatus::Disabled))
        .filter(node::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .into_iter()
        .map(|node| (node.id, node))
        .collect::<HashMap<_, _>>();
    let production_project_ids = deployments
        .iter()
        .filter(|deployment| {
            matches!(deployment.environment, DeploymentEnvironment::Production)
                && matches!(deployment.release_status, DeploymentReleaseStatus::Active)
        })
        .map(|deployment| deployment.project_id)
        .collect::<Vec<_>>();
    let bindings = project_host_binding::Entity::find()
        .filter(project_host_binding::Column::ProjectId.is_in(production_project_ids))
        .filter(project_host_binding::Column::Status.eq(HostBindingStatus::Active))
        .filter(project_host_binding::Column::Environment.ne(HostBindingEnvironment::Preview))
        .filter(project_host_binding::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let mut hosts_by_project = HashMap::<Uuid, Vec<String>>::new();
    for binding in bindings {
        hosts_by_project
            .entry(binding.project_id)
            .or_default()
            .push(binding.host);
    }

    let mut routes = Vec::new();
    for deployment in deployments {
        let Some(node_id) = deployment.serve_node_id else {
            continue;
        };
        let Some(target_node) = nodes.get(&node_id) else {
            continue;
        };
        let target_base_url = target_node
            .base_url
            .clone()
            .ok_or_else(|| AppError::Internal {
                op: OP,
                message: "assigned Serve Node has no public base URL".to_owned(),
            })?;
        let metadata_error = || AppError::Internal {
            op: OP,
            message: "assigned deployment has invalid Serve resource metadata".to_owned(),
        };
        let resources = ServeResources {
            cpu_millicores: u64::try_from(deployment.serve_cpu_millicores)
                .map_err(|_| metadata_error())?,
            memory_mb: u64::try_from(deployment.serve_memory_mb).map_err(|_| metadata_error())?,
            disk_mb: u64::try_from(deployment.serve_disk_mb).map_err(|_| metadata_error())?,
        };
        let mut deployment_hosts = deployment.preview_host.into_iter().collect::<Vec<_>>();
        if matches!(deployment.environment, DeploymentEnvironment::Production)
            && matches!(deployment.release_status, DeploymentReleaseStatus::Active)
        {
            deployment_hosts.extend(
                hosts_by_project
                    .remove(&deployment.project_id)
                    .unwrap_or_default(),
            );
        }
        routes.extend(deployment_hosts.into_iter().map(|host| ServeRoute {
            host,
            deployment_id: deployment.id,
            target_node_id: node_id,
            target_base_url: target_base_url.clone(),
            resources,
        }));
    }
    routes.sort_by(|left, right| left.host.cmp(&right.host));
    let revision = route_revision(&routes);
    Ok(ok_response(RouteSnapshotResponse { revision, routes }))
}

async fn artifact_available(
    db: &sea_orm::DatabaseConnection,
    deployment_id: Uuid,
) -> anyhow::Result<bool> {
    Ok(deployment_artifact::Entity::find()
        .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
        .filter(deployment_artifact::Column::Kind.eq(DeploymentArtifactKind::GrassOutput))
        .one(db)
        .await?
        .is_some())
}

/// GET /api/v1/internal/serve/resolve-host?host=...
///
/// Maps a public Host header to the deployment that should serve it:
/// production bindings resolve to the active production deployment only;
/// preview hosts resolve to their ready preview deployment.
pub async fn resolve_host(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Query(query): Query<ResolveHostQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.resolve_host";
    let db = super::database(&state, OP)?;

    let host =
        grass_validator::normalize_host(&query.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;

    // Production bindings first: stable domains only ever serve the active
    // production deployment.
    if let Some(binding) = hosts::find_binding_by_host(db, &host)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        if !matches!(binding.status, HostBindingStatus::Active) {
            return Err(AppError::NotFound {
                op: OP,
                message: "host binding is not active".to_owned(),
            });
        }
        if matches!(binding.environment, HostBindingEnvironment::Preview) {
            return Err(AppError::NotFound {
                op: OP,
                message: "preview-only bindings resolve through deployment hosts".to_owned(),
            });
        }

        let active =
            deployments::find_active(db, binding.project_id, DeploymentEnvironment::Production)
                .await
                .map_err(|source| AppError::Infrastructure { op: OP, source })?
                .ok_or_else(|| AppError::NotFound {
                    op: OP,
                    message: "no active production deployment for this host".to_owned(),
                })?;

        let available = artifact_available(db, active.id)
            .await
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        return Ok(ok_response(ResolveHostResponse {
            deployment_id: active.id,
            project_id: active.project_id,
            environment: "production".to_owned(),
            artifact_available: available,
        }));
    }

    // Preview hosts are stored directly on the deployment row.
    let deployment = deployments::find_by_preview_host(db, &host)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "host is not bound".to_owned(),
        })?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready)
        || !matches!(deployment.serve_status, DeploymentServeStatus::Ready)
    {
        return Err(AppError::NotFound {
            op: OP,
            message: "preview deployment is not ready".to_owned(),
        });
    }

    let available = artifact_available(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(ResolveHostResponse {
        deployment_id: deployment.id,
        project_id: deployment.project_id,
        environment: "preview".to_owned(),
        artifact_available: available,
    }))
}

#[cfg(test)]
mod tests {
    use grass_node_protocol::{
        ReportServeStatusRequest, ReportedServeStatus, ServeResources, ServeRoute,
    };
    use uuid::Uuid;

    use super::{route_revision, validate_status_report};

    #[test]
    fn serve_failure_reports_require_bounded_stable_details() {
        let valid = ReportServeStatusRequest {
            status: ReportedServeStatus::Failed,
            failure_code: Some("checksum_mismatch".to_owned()),
            failure_message: Some("downloaded artifact did not match metadata".to_owned()),
        };
        assert!(validate_status_report(&valid).is_ok());

        let mut invalid = valid.clone();
        invalid.failure_code = Some("Checksum Mismatch".to_owned());
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failure_code must be a lowercase identifier of at most 64 bytes"
        );

        invalid = valid.clone();
        invalid.failure_code = None;
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failed status requires failure_code"
        );

        invalid = valid;
        invalid.failure_message = Some("x".repeat(1025));
        assert_eq!(
            validate_status_report(&invalid).unwrap_err(),
            "failure_message must be at most 1024 bytes"
        );
    }

    #[test]
    fn non_failure_serve_reports_reject_failure_details() {
        let report = ReportServeStatusRequest {
            status: ReportedServeStatus::Ready,
            failure_code: Some("unexpected".to_owned()),
            failure_message: None,
        };

        assert_eq!(
            validate_status_report(&report).unwrap_err(),
            "failure details are only allowed for failed status"
        );
    }

    #[test]
    fn route_revision_is_order_independent_and_content_addressed() {
        let resources = ServeResources {
            cpu_millicores: 50,
            memory_mb: 64,
            disk_mb: 256,
        };
        let first = ServeRoute {
            host: "a.example.com".to_owned(),
            deployment_id: Uuid::now_v7(),
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-a:8080".to_owned(),
            resources,
        };
        let second = ServeRoute {
            host: "b.example.com".to_owned(),
            deployment_id: Uuid::now_v7(),
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-b:8080".to_owned(),
            resources,
        };

        let original = route_revision(&[first.clone(), second.clone()]);
        assert_eq!(original, route_revision(&[second.clone(), first.clone()]));
        let mut changed = second;
        changed.target_base_url = "http://node-b:9090".to_owned();
        assert_ne!(original, route_revision(&[first, changed]));
    }
}
