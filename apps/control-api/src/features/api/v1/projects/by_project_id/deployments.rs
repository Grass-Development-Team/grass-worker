use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::{
    domain::{
        delivery,
        deployment_placement::{
            create_placed_deployment, environment_gets_preview_host, preview_host_for_project,
        },
        deployments::{self, BuildTransition, CreateDeploymentParams, DeploymentListFilter},
        hosts, projects,
        quotas::QuotaDimension,
        screenshots::{self, ScreenshotState},
        source_credentials,
    },
    infra::{
        database::entity::{
            DeploymentBuildStatus, DeploymentEnvironment, DeploymentReleaseStatus,
            HostBindingEnvironment, HostBindingStatus, deployment, node, project_host_binding,
            user,
        },
        error::{AppError, ok_response},
        http::{deployment_errors::map_delivery_error, extractors::Session},
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) mod by_deployment_id;

#[derive(serde::Serialize)]
struct ListResponse {
    deployments: Vec<DeploymentResponse>,
}

#[derive(serde::Serialize)]
struct SingleDeploymentResponse {
    deployment: DeploymentResponse,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new()
        .route(
            "/projects/{project_id}/deployments",
            axum::routing::get(list).post(create),
        )
        .merge(by_deployment_id::router())
}

fn parse_environment(value: &str, op: &'static str) -> Result<DeploymentEnvironment, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "production" => Ok(DeploymentEnvironment::Production),
        "preview" => Ok(DeploymentEnvironment::Preview),
        other => Err(AppError::Validation {
            op,
            message: format!("invalid environment: {other}"),
        }),
    }
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

async fn load_users(
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

// --- Create -----------------------------------------------------------------
#[derive(Deserialize)]
struct CreateDeploymentRequest {
    #[serde(default = "default_environment")]
    environment: String,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    commit_hash: Option<String>,
    #[serde(default)]
    commit_message: Option<String>,
    #[serde(default)]
    serve_node_id: Option<Uuid>,
    #[serde(default)]
    region: Option<String>,
}

fn default_environment() -> String {
    "production".to_owned()
}

/// POST /api/v1/projects/{project_id}/deployments
async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateDeploymentRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.create";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_member(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

    projects::ensure_deployable(&access.project).map_err(|error| AppError::Conflict {
        op: OP,
        message: error.to_string(),
    })?;
    if access
        .project
        .repository_url
        .as_deref()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Err(AppError::Validation {
            op: OP,
            message: "project has no repository URL configured".to_owned(),
        });
    }
    let environment = parse_environment(&body.environment, OP)?;
    let requested_region = body
        .region
        .as_deref()
        .map(grass_validator::normalize_region)
        .transpose()
        .map_err(|error| AppError::Validation {
            op: OP,
            message: format!("region: {error}"),
        })?;

    let quota = QuotaService::new(db, cache);
    let reservation = quota
        .reserve(
            OP,
            &access.team,
            Some(session.data.user_id),
            &[QuotaCharge::one(QuotaDimension::DeploymentsMonthly)],
        )
        .await?;

    // Every deployment gets a protected moderation host when an auto-assign
    // source exists. Production bindings remain inactive until promotion.
    let preview_host = if environment_gets_preview_host(&environment) {
        preview_host_for_project(db, &access.project).await
    } else {
        None
    };
    let source_credential_version_id =
        match source_credentials::current_version_for_project(db, &access.project).await {
            Ok(version_id) => version_id,
            Err(error) => {
                quota.rollback(reservation).await;
                return Err(AppError::Conflict {
                    op: OP,
                    message: error.to_string(),
                });
            }
        };
    if access
        .project
        .repository_url
        .as_deref()
        .and_then(|url| grass_git_source::parse_repository_url(url).ok())
        .is_some_and(|endpoint| endpoint.transport == grass_git_source::GitTransport::Ssh)
        && source_credential_version_id.is_none()
    {
        quota.rollback(reservation).await;
        return Err(AppError::Validation {
            op: OP,
            message: "SSH repositories require a bound source credential".to_owned(),
        });
    }

    let deployment = match create_placed_deployment(
        db,
        CreateDeploymentParams {
            project: access.project.clone(),
            environment,
            triggered_by_user_id: Some(session.data.user_id),
            branch: optional_trimmed(body.branch),
            commit_hash: optional_trimmed(body.commit_hash),
            commit_message: optional_trimmed(body.commit_message),
            preview_host,
            source_credential_version_id,
        },
        body.serve_node_id,
        requested_region.as_deref(),
        OP,
    )
    .await
    {
        Ok(deployment) => deployment,
        Err(error) => {
            quota.rollback(reservation).await;
            return Err(error);
        }
    };
    quota
        .commit(OP, reservation, "deployment", Some(deployment.id))
        .await?;

    // Non-static runtimes are reserved but not implemented: fail the
    // deployment immediately with the stable message instead of letting it
    // sit in the queue forever.
    let deployment = match deployments::runtime_failure(&deployment.runtime_kind) {
        Some((code, message)) => {
            let transaction = crate::infra::audit::AuditTransaction::begin(db)
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
            let deployment = delivery::transition_unsuccessful_build(
                &transaction,
                deployment,
                BuildTransition {
                    to: DeploymentBuildStatus::Failed,
                    stage: None,
                    failure_code: Some(code.to_owned()),
                    failure_message: Some(message.to_owned()),
                    build_node_id: None,
                },
            )
            .await
            .map_err(|error| map_delivery_error(error, OP))?;
            transaction
                .commit()
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
            deployment
        }
        None => deployment,
    };

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let users = load_users(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;

    Ok(ok_response(SingleDeploymentResponse {
        deployment: deployment_view(&deployment, &urls, &preview_ids, &users, &nodes),
    }))
}

// --- List / detail ----------------------------------------------------------
#[derive(Deserialize)]
struct ListDeploymentsQuery {
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    build_status: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    offset: Option<u64>,
}

/// GET /api/v1/projects/{project_id}/deployments
async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListDeploymentsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.list";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;

    let environment = match &query.environment {
        Some(value) => Some(parse_environment(value, OP)?),
        None => None,
    };
    let build_status = match &query.build_status {
        Some(value) => Some(parse_build_status(value, OP)?),
        None => None,
    };

    let deployments = deployments::list_for_project(
        db,
        access.project.id,
        DeploymentListFilter {
            environment,
            build_status,
            limit: query.limit.unwrap_or(50),
            offset: query.offset.unwrap_or(0),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let users = load_users(db, &deployments)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, &deployments)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;
    let deployment_ids = deployments.iter().map(|item| item.id).collect::<Vec<_>>();
    let screenshot_states = screenshots::states_for(db, &deployment_ids)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let capture_configured = state.config.read().unwrap().screenshot.is_some();

    Ok(ok_response(ListResponse {
        deployments: deployments
            .iter()
            .map(|deployment| {
                let mut view = deployment_view(deployment, &urls, &preview_ids, &users, &nodes);
                attach_screenshot(
                    &mut view,
                    deployment,
                    screenshot_states.get(&deployment.id),
                    capture_configured,
                );
                view
            })
            .collect::<Vec<_>>(),
    }))
}

fn parse_build_status(value: &str, op: &'static str) -> Result<DeploymentBuildStatus, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => Ok(DeploymentBuildStatus::Pending),
        "claimed" => Ok(DeploymentBuildStatus::Claimed),
        "queued" => Ok(DeploymentBuildStatus::Queued),
        "building" => Ok(DeploymentBuildStatus::Building),
        "ready" => Ok(DeploymentBuildStatus::Ready),
        "failed" => Ok(DeploymentBuildStatus::Failed),
        "canceled" => Ok(DeploymentBuildStatus::Canceled),
        other => Err(AppError::Validation {
            op,
            message: format!("invalid build status: {other}"),
        }),
    }
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_request_defaults_to_automatic_placement() {
        let request: CreateDeploymentRequest =
            serde_json::from_str(r#"{"environment":"preview"}"#).unwrap();

        assert_eq!(request.serve_node_id, None);
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
