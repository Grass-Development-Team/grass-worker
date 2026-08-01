use std::collections::{HashMap, HashSet};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use grass_node_protocol::{
    ReportServeStatusRequest, ReportServeStatusResponse, ReportedServeStatus, ResolveHostResponse,
    RouteSnapshotResponse, ServeAccess, ServeArtifact, ServeAssignment, ServeAssignmentStatus,
    ServeAssignmentsResponse, ServeResources, ServeRoute, SsrLeaseResponse,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{delivery, deployments, hosts, projects, scheduler, ssr_leases},
    infra::{
        database::entity::{
            DeploymentArtifactKind, DeploymentBuildStatus, DeploymentEnvironment,
            DeploymentReleaseStatus, DeploymentServeStatus, HostBindingEnvironment,
            HostBindingStatus, NodeDeploymentMigrationStatus, NodeStatus, deployment,
            deployment_artifact, node, node_deployment_migration, project_host_binding,
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

fn map_lease_error(error: ssr_leases::LeaseError, op: &'static str) -> AppError {
    match error {
        ssr_leases::LeaseError::ProcessQuota => AppError::QuotaExceeded {
            op,
            message: "quota exceeded: ssr_processes limit reached".to_owned(),
        },
        ssr_leases::LeaseError::HourQuota => AppError::QuotaExceeded {
            op,
            message: "quota exceeded: ssr_hours.monthly limit reached".to_owned(),
        },
        ssr_leases::LeaseError::NotAssigned => AppError::Forbidden {
            op,
            message: "deployment is not an SSR Serve assignment for this node".to_owned(),
        },
        ssr_leases::LeaseError::WrongNode => AppError::Forbidden {
            op,
            message: "SSR lease belongs to another node".to_owned(),
        },
        ssr_leases::LeaseError::NotFound => AppError::NotFound {
            op,
            message: "SSR lease not found or expired".to_owned(),
        },
        ssr_leases::LeaseError::Database(source) => AppError::Infrastructure { op, source },
    }
}

fn lease_response(
    lease: &crate::infra::database::entity::ssr_process_lease::Model,
) -> SsrLeaseResponse {
    SsrLeaseResponse {
        lease_id: lease.id,
        expires_at_unix: lease.expires_at.unix_timestamp(),
        hour_block_start_unix: lease.hour_block_start.unix_timestamp(),
    }
}

/// POST /api/v1/internal/serve/deployments/{deployment_id}/ssr-lease
pub async fn acquire_ssr_lease(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.ssr_lease.acquire";
    ensure_serve_node(&node, OP)?;
    let db = super::database(&state, OP)?;
    let lease = ssr_leases::acquire(db, deployment_id, node.id, time::OffsetDateTime::now_utc())
        .await
        .map_err(|error| map_lease_error(error, OP))?;
    Ok(ok_response(lease_response(&lease)))
}

/// POST /api/v1/internal/serve/deployments/{deployment_id}/ssr-lease/{lease_id}/renew
pub async fn renew_ssr_lease(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path((deployment_id, lease_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.ssr_lease.renew";
    ensure_serve_node(&node, OP)?;
    let db = super::database(&state, OP)?;
    let lease = ssr_leases::renew(
        db,
        lease_id,
        deployment_id,
        node.id,
        time::OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|error| map_lease_error(error, OP))?;
    Ok(ok_response(lease_response(&lease)))
}

/// POST /api/v1/internal/serve/deployments/{deployment_id}/ssr-lease/{lease_id}/release
pub async fn release_ssr_lease(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path((deployment_id, lease_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.ssr_lease.release";
    ensure_serve_node(&node, OP)?;
    let db = super::database(&state, OP)?;
    let released = ssr_leases::release(
        db,
        lease_id,
        deployment_id,
        node.id,
        time::OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|error| map_lease_error(error, OP))?;
    Ok(ok_response(serde_json::json!({ "released": released })))
}

fn shadow_migration_statuses() -> [NodeDeploymentMigrationStatus; 3] {
    [
        NodeDeploymentMigrationStatus::Pending,
        NodeDeploymentMigrationStatus::Syncing,
        NodeDeploymentMigrationStatus::Ready,
    ]
}

fn migration_is_shadow_assignment(status: &NodeDeploymentMigrationStatus) -> bool {
    shadow_migration_statuses().contains(status)
}

pub(crate) fn migration_allows_artifact_download(status: &NodeDeploymentMigrationStatus) -> bool {
    migration_is_shadow_assignment(status)
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
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    scheduler::lock_placement(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deployment = deployments::get_by_id_for_update(&transaction, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let migration = node_deployment_migration::Entity::find()
        .filter(node_deployment_migration::Column::DeploymentId.eq(deployment_id))
        .filter(node_deployment_migration::Column::TargetNodeId.eq(node.id))
        .filter(node_deployment_migration::Column::Status.is_in(shadow_migration_statuses()))
        .lock_exclusive()
        .one(&transaction)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    if let Some(migration) = migration.filter(|_| deployment.serve_node_id != Some(node.id)) {
        let now = time::OffsetDateTime::now_utc();
        let (status, error, ready_at) = match report.status {
            ReportedServeStatus::Syncing => (NodeDeploymentMigrationStatus::Syncing, None, None),
            ReportedServeStatus::Ready => (NodeDeploymentMigrationStatus::Ready, None, Some(now)),
            ReportedServeStatus::Failed => (
                NodeDeploymentMigrationStatus::Failed,
                report.failure_message.or(report.failure_code),
                None,
            ),
        };
        let mut active: node_deployment_migration::ActiveModel = migration.into();
        active.status = sea_orm::ActiveValue::Set(status);
        active.error = sea_orm::ActiveValue::Set(error);
        active.ready_at = sea_orm::ActiveValue::Set(ready_at);
        active.updated_at = sea_orm::ActiveValue::Set(now);
        active
            .update(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        transaction
            .commit()
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
        return Ok(ok_response(ReportServeStatusResponse {
            acknowledged: true,
        }));
    }
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
        &transaction,
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
        super::deployments::auto_activate_if_allowed(&transaction, updated).await?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

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
        .filter(
            deployment::Column::ServeStatus
                .is_in([DeploymentServeStatus::Ready, DeploymentServeStatus::Retired]),
        )
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

    let mut preview_groups = HashMap::<(Uuid, bool), Vec<delivery::DeliveryCandidate>>::new();
    for deployment in &deployments {
        if deployment.preview_host.is_some() {
            preview_groups
                .entry((
                    deployment.project_id,
                    matches!(deployment.environment, DeploymentEnvironment::Production),
                ))
                .or_default()
                .push(delivery::candidate_from_model(deployment));
        }
    }
    let effective_preview_ids = preview_groups
        .values()
        .filter_map(|candidates| delivery::effective_preview_id(candidates))
        .collect::<HashSet<_>>();

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
        let mut deployment_hosts = if effective_preview_ids.contains(&deployment.id) {
            deployment
                .preview_host
                .into_iter()
                .map(|host| (host, ServeAccess::TeamOrPlatformAdmin))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if matches!(deployment.environment, DeploymentEnvironment::Production)
            && matches!(deployment.release_status, DeploymentReleaseStatus::Active)
        {
            deployment_hosts.extend(
                hosts_by_project
                    .remove(&deployment.project_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|host| (host, ServeAccess::Public)),
            );
        }
        routes.extend(
            deployment_hosts
                .into_iter()
                .map(|(host, access)| ServeRoute {
                    host,
                    deployment_id: deployment.id,
                    target_node_id: node_id,
                    target_base_url: target_base_url.clone(),
                    resources,
                    access,
                }),
        );
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
/// preview hosts resolve to their ready deployment, including production
/// deployments waiting for moderation.
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
            team_id: active.team_id,
            host,
            environment: "production".to_owned(),
            artifact_available: available,
            access: ServeAccess::Public,
        }));
    }

    // Protected preview hosts are stored directly on the deployment row.
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
    let effective =
        delivery::effective_preview(db, deployment.project_id, deployment.environment.clone())
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?;
    if effective.as_ref().map(|item| item.id) != Some(deployment.id) {
        return Err(AppError::NotFound {
            op: OP,
            message: "preview deployment has been superseded".to_owned(),
        });
    }

    let available = artifact_available(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(ResolveHostResponse {
        deployment_id: deployment.id,
        project_id: deployment.project_id,
        team_id: deployment.team_id,
        host,
        environment: deployments::environment_value(&deployment.environment).to_owned(),
        artifact_available: available,
        access: ServeAccess::TeamOrPlatformAdmin,
    }))
}

#[cfg(test)]
mod tests {
    use grass_node_protocol::{
        ReportServeStatusRequest, ReportedServeStatus, ServeAccess, ServeResources, ServeRoute,
    };
    use uuid::Uuid;

    use crate::infra::database::entity::NodeDeploymentMigrationStatus;

    use super::{
        deployments, migration_allows_artifact_download, migration_is_shadow_assignment,
        route_revision, validate_status_report,
    };

    #[test]
    fn ready_shadow_assignments_remain_authorized_until_atomic_cutover() {
        for status in [
            NodeDeploymentMigrationStatus::Pending,
            NodeDeploymentMigrationStatus::Syncing,
            NodeDeploymentMigrationStatus::Ready,
        ] {
            assert!(migration_is_shadow_assignment(&status));
            assert!(migration_allows_artifact_download(&status));
        }
        assert!(!migration_is_shadow_assignment(
            &NodeDeploymentMigrationStatus::Failed
        ));
        assert!(!migration_allows_artifact_download(
            &NodeDeploymentMigrationStatus::Failed
        ));
    }

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
            access: ServeAccess::Public,
        };
        let second = ServeRoute {
            host: "b.example.com".to_owned(),
            deployment_id: Uuid::now_v7(),
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-b:8080".to_owned(),
            resources,
            access: ServeAccess::TeamOrPlatformAdmin,
        };

        let original = route_revision(&[first.clone(), second.clone()]);
        assert_eq!(original, route_revision(&[second.clone(), first.clone()]));
        let mut changed = second;
        changed.target_base_url = "http://node-b:9090".to_owned();
        assert_ne!(original, route_revision(&[first, changed]));
    }
}
