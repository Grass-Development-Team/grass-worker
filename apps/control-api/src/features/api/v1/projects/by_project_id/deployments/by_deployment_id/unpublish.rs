use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{delivery, deployments, hosts, projects},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{
            AuditEventResult, DeploymentEnvironment, DeploymentReleaseStatus,
            HostBindingEnvironment, HostBindingStatus, deployment, node, project_host_binding,
            user,
        },
        error::{AppError, ok_response},
        http::{deployment_errors::map_delivery_error, extractors::Session},
        route_invalidation,
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct SingleDeploymentResponse {
    deployment: DeploymentResponse,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/unpublish",
        axum::routing::post(unpublish),
    )
}

// --- DTO --------------------------------------------------------------------
struct UrlContext {
    production_host: Option<String>,
    public_scheme: &'static str,
}

impl UrlContext {
    pub(crate) async fn load(
        db: &sea_orm::DatabaseConnection,
        project_id: Uuid,
    ) -> anyhow::Result<Self> {
        let bindings = hosts::list_bindings_for_project(db, project_id).await?;
        let production_host = bindings
            .iter()
            .filter(|binding| {
                matches!(binding.status, HostBindingStatus::Active)
                    && matches!(
                        binding.environment,
                        HostBindingEnvironment::Production | HostBindingEnvironment::All
                    )
            })
            .max_by_key(|binding| binding.is_primary)
            .map(|binding: &project_host_binding::Model| binding.host.clone());

        Ok(Self {
            production_host,
            // First-stage serve is plain HTTP unless a proxy terminates TLS.
            public_scheme: "http",
        })
    }

    fn urls(
        &self,
        deployment: &deployment::Model,
        preview_available: bool,
    ) -> (Option<String>, Option<String>) {
        let preview_url = preview_available
            .then(|| {
                deployment
                    .preview_host
                    .as_ref()
                    .map(|host| format!("{}://{}", self.public_scheme, host))
            })
            .flatten();
        let production_url =
            production_url_is_available(&deployment.environment, &deployment.release_status)
                .then(|| {
                    self.production_host
                        .as_ref()
                        .map(|host| format!("{}://{}", self.public_scheme, host))
                })
                .flatten();

        (preview_url, production_url)
    }
}

fn production_url_is_available(
    environment: &DeploymentEnvironment,
    release_status: &DeploymentReleaseStatus,
) -> bool {
    matches!(environment, DeploymentEnvironment::Production)
        && matches!(release_status, DeploymentReleaseStatus::Active)
}

#[derive(Serialize)]
struct DeploymentResponse {
    id: Uuid,
    project_id: Uuid,
    team_id: Uuid,
    region: String,
    build_node: Option<DeploymentNodeResponse>,
    serve_node: Option<DeploymentNodeResponse>,
    environment: &'static str,
    runtime_kind: &'static str,
    build_status: &'static str,
    serve_status: &'static str,
    release_status: &'static str,
    release_pending: bool,
    pending_release_reason: Option<&'static str>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    pending_release_requested_at: Option<time::OffsetDateTime>,
    serve_resources: DeploymentResourcesResponse,
    overcommitted: bool,
    build_stage: Option<String>,
    source: DeploymentSourceResponse,
    triggered_by: Option<DeploymentActorResponse>,
    failure_code: Option<String>,
    failure_message: Option<String>,
    serve_failure_code: Option<String>,
    serve_failure_message: Option<String>,
    duration_seconds: Option<i64>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    claimed_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    build_started_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    build_finished_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    serve_started_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    serve_finished_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    screenshot_status: &'static str,
    screenshot_url: Option<String>,
    preview_url: Option<String>,
    production_url: Option<String>,
}

#[derive(Serialize)]
struct DeploymentNodeResponse {
    id: Uuid,
    name: String,
}

#[derive(Serialize)]
struct DeploymentActorResponse {
    id: Uuid,
    email: String,
    display_name: Option<String>,
}

#[derive(Serialize)]
struct DeploymentResourcesResponse {
    cpu_millicores: i64,
    memory_mb: i64,
    disk_mb: i64,
}

#[derive(Serialize)]
struct DeploymentSourceResponse {
    repository_url: Option<String>,
    branch: Option<String>,
    commit_hash: Option<String>,
    commit_message: Option<String>,
}

fn deployment_view(
    deployment: &deployment::Model,
    urls: &UrlContext,
    effective_preview_ids: &HashSet<Uuid>,
    users: &HashMap<Uuid, user::Model>,
    nodes: &HashMap<Uuid, node::Model>,
) -> DeploymentResponse {
    let duration_seconds = match (deployment.build_started_at, deployment.build_finished_at) {
        (Some(started), Some(finished)) => Some((finished - started).whole_seconds().max(0)),
        _ => None,
    };
    let triggered_by = deployment
        .triggered_by_user_id
        .and_then(|id| users.get(&id))
        .map(|user| DeploymentActorResponse {
            id: user.id,
            email: user.email.clone(),
            display_name: user.display_name.clone(),
        });
    let node_view = |id: Option<Uuid>| {
        id.and_then(|id| nodes.get(&id))
            .map(|node| DeploymentNodeResponse {
                id: node.id,
                name: node.name.clone(),
            })
    };
    let (preview_url, production_url) =
        urls.urls(deployment, effective_preview_ids.contains(&deployment.id));
    DeploymentResponse {
        id: deployment.id,
        project_id: deployment.project_id,
        team_id: deployment.team_id,
        region: deployment.region.clone(),
        build_node: node_view(deployment.build_node_id),
        serve_node: node_view(deployment.serve_node_id),
        environment: deployments::environment_value(&deployment.environment),
        runtime_kind: projects::runtime_value(&deployment.runtime_kind),
        build_status: deployments::build_status_value(&deployment.build_status),
        serve_status: deployments::serve_status_value(&deployment.serve_status),
        release_status: deployments::release_status_value(&deployment.release_status),
        release_pending: deployment.pending_release_reason.is_some(),
        pending_release_reason: deployment
            .pending_release_reason
            .as_ref()
            .map(deployments::release_reason_value),
        pending_release_requested_at: deployment.pending_release_requested_at,
        serve_resources: DeploymentResourcesResponse {
            cpu_millicores: deployment.serve_cpu_millicores,
            memory_mb: deployment.serve_memory_mb,
            disk_mb: deployment.serve_disk_mb,
        },
        overcommitted: deployment.overcommitted,
        build_stage: deployment.build_stage.clone(),
        source: DeploymentSourceResponse {
            repository_url: deployment.source_repository_url.clone(),
            branch: deployment.source_branch.clone(),
            commit_hash: deployment.commit_hash.clone(),
            commit_message: deployment.commit_message.clone(),
        },
        triggered_by,
        failure_code: deployment.failure_code.clone(),
        failure_message: deployment.failure_message.clone(),
        serve_failure_code: deployment.serve_failure_code.clone(),
        serve_failure_message: deployment.serve_failure_message.clone(),
        duration_seconds,
        claimed_at: deployment.claimed_at,
        build_started_at: deployment.build_started_at,
        build_finished_at: deployment.build_finished_at,
        serve_started_at: deployment.serve_started_at,
        serve_finished_at: deployment.serve_finished_at,
        created_at: deployment.created_at,
        screenshot_status: "unavailable",
        screenshot_url: None,
        preview_url,
        production_url,
    }
}

async fn effective_preview_ids(
    db: &sea_orm::DatabaseConnection,
    project_id: Uuid,
    op: &'static str,
) -> Result<HashSet<Uuid>, AppError> {
    let mut ids = HashSet::new();
    for environment in [
        DeploymentEnvironment::Production,
        DeploymentEnvironment::Preview,
    ] {
        if let Some(deployment) = delivery::effective_preview(db, project_id, environment)
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?
        {
            ids.insert(deployment.id);
        }
    }
    Ok(ids)
}

async fn load_nodes(
    db: &sea_orm::DatabaseConnection,
    deployments: &[deployment::Model],
) -> anyhow::Result<HashMap<Uuid, node::Model>> {
    let node_ids: Vec<Uuid> = deployments
        .iter()
        .flat_map(|deployment| [deployment.build_node_id, deployment.serve_node_id])
        .flatten()
        .collect();
    if node_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(node::Entity::find()
        .filter(node::Column::Id.is_in(node_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|node| (node.id, node))
        .collect())
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/unpublish
async fn unpublish(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.unpublish";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_admin(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
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
        .filter(|deployment| deployment.project_id == access.project.id)
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "deployment not found".to_owned(),
        })?;
    let deployment = delivery::remove_publication(
        &transaction,
        deployment,
        delivery::PublicationRemovalKind::TeamUser,
    )
    .await
    .map_err(|error| map_delivery_error(error, OP))?;
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(access.team.id),
            action: "deployment.unpublished".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "project_id": project_id }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let secret_key = state.config.read().unwrap().secrets.secret_key.clone();
    route_invalidation::invalidate_deployment_best_effort(db, &secret_key, deployment.id, OP).await;

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;
    Ok(ok_response(SingleDeploymentResponse {
        deployment: deployment_view(&deployment, &urls, &preview_ids, &HashMap::new(), &nodes),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_urls_are_only_exposed_for_active_production_deployments() {
        assert!(production_url_is_available(
            &DeploymentEnvironment::Production,
            &DeploymentReleaseStatus::Active,
        ));
        assert!(!production_url_is_available(
            &DeploymentEnvironment::Preview,
            &DeploymentReleaseStatus::Active,
        ));
        assert!(!production_url_is_available(
            &DeploymentEnvironment::Production,
            &DeploymentReleaseStatus::Approved,
        ));
    }

    #[test]
    fn deployment_response_preserves_the_existing_wire_shape() {
        let deployment = crate::test_support::ready_deployment();
        let urls = UrlContext {
            production_host: Some("live.test".to_owned()),
            public_scheme: "http",
        };
        let response = deployment_view(
            &deployment,
            &urls,
            &HashSet::from([deployment.id]),
            &HashMap::new(),
            &HashMap::new(),
        );
        let expected: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/deployment-response.json"
        )))
        .unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), expected);
    }
}
