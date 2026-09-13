use axum::{Extension, extract::State, response::IntoResponse};
use grass_node_protocol::{ServeAccess, ServeResources, ServeRoute};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{
    domain::delivery,
    infra::{
        database::entity::{
            DeploymentBuildStatus, DeploymentEnvironment, DeploymentReleaseStatus,
            DeploymentServeStatus, HostBindingEnvironment, HostBindingKind, HostBindingStatus,
            NodeStatus, deployment, node, project_host_binding,
        },
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RouteSnapshotResponse {
    pub revision: String,
    pub routes: Vec<ServeRoute>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/routes", axum::routing::get(routes))
}

fn route_revision(routes: &[ServeRoute]) -> String {
    let mut canonical = routes.iter().collect::<Vec<_>>();
    canonical.sort_by(|left, right| {
        (
            &left.host,
            &left.region,
            left.deployment_id,
            left.target_node_id,
            &left.target_base_url,
        )
            .cmp(&(
                &right.host,
                &right.region,
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

/// GET /api/v1/internal/serve/routes
pub async fn routes(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(requesting_node)): Extension<AuthenticatedNode>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.routes";
    ensure_serve_node(&requesting_node, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
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
        if matches!(binding.kind, HostBindingKind::Custom)
            && !crate::domain::certificates::binding_eligible(&binding)
        {
            continue;
        }
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
                    region: deployment.region.clone(),
                    deployment_id: deployment.id,
                    target_node_id: node_id,
                    target_base_url: target_base_url.clone(),
                    gateway_authentication: crate::domain::nodes::gateway_authentication(
                        target_node,
                    ),
                    resources,
                    access,
                }),
        );
    }
    routes.sort_by(|left, right| left.host.cmp(&right.host));
    let revision = route_revision(&routes);
    Ok(ok_response(RouteSnapshotResponse { revision, routes }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_node_protocol::ServeAccess;
    use grass_node_protocol::ServeResources;
    use grass_node_protocol::ServeRoute;
    use uuid::Uuid;
    #[test]
    fn route_revision_is_order_independent_and_content_addressed() {
        let resources = ServeResources {
            cpu_millicores: 50,
            memory_mb: 64,
            disk_mb: 256,
        };
        let first = ServeRoute {
            host: "a.example.com".to_owned(),
            region: "default".to_owned(),
            deployment_id: Uuid::now_v7(),
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-a:8080".to_owned(),
            gateway_authentication: Default::default(),
            resources,
            access: ServeAccess::Public,
        };
        let second = ServeRoute {
            host: "b.example.com".to_owned(),
            region: "default".to_owned(),
            deployment_id: Uuid::now_v7(),
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-b:8080".to_owned(),
            gateway_authentication: Default::default(),
            resources,
            access: ServeAccess::TeamOrPlatformAdmin,
        };

        let original = route_revision(&[first.clone(), second.clone()]);
        assert_eq!(original, route_revision(&[second.clone(), first.clone()]));
        let mut changed = second;
        changed.target_base_url = "http://node-b:9090".to_owned();
        assert_ne!(original, route_revision(&[first, changed]));
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            RouteSnapshotResponse,
            grass_node_protocol::RouteSnapshotResponse,
        >("RouteSnapshotResponse");
    }
}
