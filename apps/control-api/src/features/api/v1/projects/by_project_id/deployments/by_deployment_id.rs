use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{
    domain::{
        delivery,
        deployments::{self, ReviewMode},
        hosts, projects,
        screenshots::{self, ScreenshotState},
    },
    infra::{
        database::entity::{
            DeploymentEnvironment, DeploymentReleaseStatus, HostBindingEnvironment,
            HostBindingStatus, deployment, node, project_host_binding, user,
        },
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) mod artifacts;
pub(crate) mod build_log;
pub(crate) mod cancel;
pub(crate) mod events;
pub(crate) mod logs;
pub(crate) mod promote;
pub(crate) mod retry;
pub(crate) mod rollback;
pub(crate) mod screenshot;
pub(crate) mod unpublish;

#[derive(serde::Serialize)]
struct DetailResponse {
    deployment: DeploymentResponse,
    events: Vec<EventResponse>,
    artifacts: Vec<ArtifactResponse>,
    reviews: Vec<ReviewResponse>,
    review_required: bool,
    was_active: bool,
}

#[derive(serde::Serialize)]
struct ArtifactResponse {
    id: Uuid,
    kind: &'static str,
    storage_path: String,
    checksum_sha256: Option<String>,
    size_bytes: Option<i64>,
    manifest: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct EventResponse {
    id: Uuid,
    kind: &'static str,
    message: String,
    metadata: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ReviewResponse {
    id: Uuid,
    status: &'static str,
    reviewer_user_id: Option<Uuid>,
    reason: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    requested_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    reviewed_at: Option<time::OffsetDateTime>,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new()
        .route(
            "/projects/{project_id}/deployments/{deployment_id}",
            axum::routing::get(detail),
        )
        .merge(artifacts::router())
        .merge(build_log::router())
        .merge(cancel::router())
        .merge(events::router())
        .merge(logs::router())
        .merge(promote::router())
        .merge(retry::router())
        .merge(rollback::router())
        .merge(screenshot::router())
        .merge(unpublish::router())
}

// --- DTO --------------------------------------------------------------------
pub(crate) struct UrlContext {
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

fn attach_screenshot(
    view: &mut DeploymentResponse,
    deployment: &deployment::Model,
    state: Option<&ScreenshotState>,
    capture_configured: bool,
) {
    let effective = state.copied().unwrap_or_else(|| {
        if capture_configured && screenshots::eligible(deployment) {
            ScreenshotState::Pending
        } else {
            ScreenshotState::Unavailable
        }
    });
    let (status, url) = match effective {
        ScreenshotState::Pending => ("pending", None),
        ScreenshotState::Ready(_) => (
            "ready",
            Some(format!(
                "/api/v1/projects/{}/deployments/{}/screenshot",
                deployment.project_id, deployment.id,
            )),
        ),
        ScreenshotState::Unavailable => ("unavailable", None),
    };
    view.screenshot_status = status;
    view.screenshot_url = url;
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

pub(crate) async fn load_users(
    db: &sea_orm::DatabaseConnection,
    deployments: &[deployment::Model],
) -> anyhow::Result<HashMap<Uuid, user::Model>> {
    let user_ids: Vec<Uuid> = deployments
        .iter()
        .filter_map(|deployment| deployment.triggered_by_user_id)
        .collect();
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(user::Entity::find()
        .filter(user::Column::Id.is_in(user_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|user| (user.id, user))
        .collect())
}

pub(crate) async fn load_nodes(
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

async fn load_deployment(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        });
    }
    Ok(deployment)
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.detail";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let users = load_users(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let events = deployments::list_events(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let artifacts = deployments::list_artifacts(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let reviews = deployments::list_reviews(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let was_active = deployments::was_active(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let policy = deployments::review_policy_for_team(db, deployment.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;
    let screenshot_states = screenshots::states_for(db, &[deployment.id])
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let mut deployment_view = deployment_view(&deployment, &urls, &preview_ids, &users, &nodes);
    attach_screenshot(
        &mut deployment_view,
        &deployment,
        screenshot_states.get(&deployment.id),
        state.config.read().unwrap().screenshot.is_some(),
    );

    Ok(ok_response(DetailResponse {
        deployment: deployment_view,
        events: events
            .iter()
            .map(|event| EventResponse {
                id: event.id,
                kind: event_kind_value(&event.kind),
                message: event.message.clone(),
                metadata: event.metadata.clone(),
                created_at: event.created_at,
            })
            .collect::<Vec<_>>(),
        artifacts: artifacts
            .iter()
            .map(|artifact| ArtifactResponse {
                id: artifact.id,
                kind: artifact_kind_value(&artifact.kind),
                storage_path: artifact.storage_path.clone(),
                checksum_sha256: artifact.checksum_sha256.clone(),
                size_bytes: artifact.size_bytes,
                manifest: artifact.manifest.clone(),
                created_at: artifact.created_at,
            })
            .collect::<Vec<_>>(),
        reviews: reviews
            .iter()
            .map(|review| ReviewResponse {
                id: review.id,
                status: review_status_value(&review.status),
                reviewer_user_id: review.reviewer_user_id,
                reason: review.reason.clone(),
                requested_at: review.requested_at,
                reviewed_at: review.reviewed_at,
            })
            .collect::<Vec<_>>(),
        review_required: matches!(policy.mode_for(&deployment.environment), ReviewMode::Manual),
        was_active,
    }))
}

fn event_kind_value(kind: &crate::infra::database::entity::DeploymentEventKind) -> &'static str {
    use crate::infra::database::entity::DeploymentEventKind as K;
    match kind {
        K::System => "system",
        K::Build => "build",
        K::Serve => "serve",
        K::Release => "release",
        K::Review => "review",
        K::Host => "host",
    }
}

fn artifact_kind_value(
    kind: &crate::infra::database::entity::DeploymentArtifactKind,
) -> &'static str {
    use crate::infra::database::entity::DeploymentArtifactKind as K;
    match kind {
        K::GrassOutput => "grass_output",
        K::BuildLog => "build_log",
        K::StaticSite => "static_site",
        K::Screenshot => "screenshot",
    }
}

fn review_status_value(
    status: &crate::infra::database::entity::DeploymentReviewStatus,
) -> &'static str {
    use crate::infra::database::entity::DeploymentReviewStatus as S;
    match status {
        S::Pending => "pending",
        S::Approved => "approved",
        S::Rejected => "rejected",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn screenshot_fields_follow_capture_state_without_losing_nulls() {
        let deployment = crate::test_support::ready_deployment();
        let urls = UrlContext {
            production_host: None,
            public_scheme: "http",
        };
        let mut response = deployment_view(
            &deployment,
            &urls,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        attach_screenshot(&mut response, &deployment, None, true);
        assert_eq!(response.screenshot_status, "pending");
        assert!(response.screenshot_url.is_none());
        attach_screenshot(
            &mut response,
            &deployment,
            Some(&ScreenshotState::Ready(Uuid::nil())),
            true,
        );
        assert_eq!(response.screenshot_status, "ready");
        assert!(
            response
                .screenshot_url
                .as_deref()
                .unwrap()
                .ends_with("/screenshot")
        );
        attach_screenshot(
            &mut response,
            &deployment,
            Some(&ScreenshotState::Unavailable),
            true,
        );
        let serialized = serde_json::to_value(response).unwrap();
        assert_eq!(serialized["screenshot_status"], "unavailable");
        assert!(
            serialized
                .as_object()
                .unwrap()
                .contains_key("screenshot_url")
        );
        assert!(serialized["screenshot_url"].is_null());
    }
}
