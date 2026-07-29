pub mod logs;

use std::collections::{HashMap, HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        delivery::{self, DeliveryError, ReleaseRequestOutcome},
        deployments::{
            self, BuildTransition, CreateDeploymentParams, DeploymentListFilter,
            DeploymentStateError, ReviewMode,
        },
        hosts, projects,
        quotas::QuotaDimension,
        scheduler::{self, PlacementMode, ScheduleError},
        source_credentials,
    },
    infra::{
        database::entity::{
            AuditEventResult, AuditEventVisibility, DeploymentBuildStatus, DeploymentEnvironment,
            DeploymentReleaseStatus, HostBindingEnvironment, HostBindingStatus, ReleaseReason,
            deployment, node, project_host_binding, user,
        },
        error::{AppError, accepted_response, ok_response},
        http::extractors::Session,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn map_state_error(error: DeploymentStateError, op: &'static str) -> AppError {
    match error {
        DeploymentStateError::InvalidBuildTransition { .. }
        | DeploymentStateError::InvalidReleaseTransition { .. }
        | DeploymentStateError::InvalidServeTransition { .. }
        | DeploymentStateError::BuildNotReady
        | DeploymentStateError::ServeNotReady => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        DeploymentStateError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
    }
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

/// Cache key checked by Nodes to learn a build was canceled server-side.
pub(crate) fn cancel_flag_key(deployment_id: Uuid) -> String {
    format!("deployment:{deployment_id}:cancel")
}

pub(crate) fn cancellation_was_requested(
    status: &DeploymentBuildStatus,
    cancel_flagged: bool,
) -> bool {
    matches!(status, DeploymentBuildStatus::Canceled) || cancel_flagged
}

fn cancellation_releases_build_slot(status: &DeploymentBuildStatus) -> bool {
    matches!(
        status,
        DeploymentBuildStatus::Claimed
            | DeploymentBuildStatus::Queued
            | DeploymentBuildStatus::Building
    )
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

    fn urls(&self, deployment: &deployment::Model, preview_available: bool) -> serde_json::Value {
        let preview_url = preview_available
            .then(|| {
                deployment
                    .preview_host
                    .as_ref()
                    .map(|host| format!("{}://{}", self.public_scheme, host))
            })
            .flatten();
        let production_url = matches!(deployment.release_status, DeploymentReleaseStatus::Active)
            .then(|| {
                self.production_host
                    .as_ref()
                    .map(|host| format!("{}://{}", self.public_scheme, host))
            })
            .flatten();

        json!({
            "preview_url": preview_url,
            "production_url": production_url,
        })
    }
}

pub(crate) fn deployment_view(
    deployment: &deployment::Model,
    urls: &UrlContext,
    effective_preview_ids: &HashSet<Uuid>,
    users: &HashMap<Uuid, user::Model>,
    nodes: &HashMap<Uuid, node::Model>,
) -> serde_json::Value {
    let duration_seconds = match (deployment.build_started_at, deployment.build_finished_at) {
        (Some(started), Some(finished)) => Some((finished - started).whole_seconds().max(0)),
        _ => None,
    };
    let triggered_by = deployment
        .triggered_by_user_id
        .and_then(|user_id| users.get(&user_id))
        .map(|user| {
            json!({
                "id": user.id,
                "email": user.email,
                "display_name": user.display_name,
            })
        });
    let node_view = |node_id: Option<Uuid>| {
        node_id.and_then(|node_id| {
            nodes.get(&node_id).map(|node| {
                json!({
                    "id": node.id,
                    "name": node.name,
                })
            })
        })
    };

    let mut view = json!({
        "id": deployment.id,
        "project_id": deployment.project_id,
        "team_id": deployment.team_id,
        "build_node": node_view(deployment.build_node_id),
        "serve_node": node_view(deployment.serve_node_id),
        "environment": deployments::environment_value(&deployment.environment),
        "runtime_kind": projects::runtime_value(&deployment.runtime_kind),
        "build_status": deployments::build_status_value(&deployment.build_status),
        "serve_status": deployments::serve_status_value(&deployment.serve_status),
        "release_status": deployments::release_status_value(&deployment.release_status),
        "release_pending": deployment.pending_release_reason.is_some(),
        "pending_release_reason": deployment
            .pending_release_reason
            .as_ref()
            .map(deployments::release_reason_value),
        "pending_release_requested_at": ts(deployment.pending_release_requested_at),
        "serve_resources": {
            "cpu_millicores": deployment.serve_cpu_millicores,
            "memory_mb": deployment.serve_memory_mb,
            "disk_mb": deployment.serve_disk_mb,
        },
        "overcommitted": deployment.overcommitted,
        "build_stage": deployment.build_stage,
        "source": {
            "repository_url": deployment.source_repository_url,
            "branch": deployment.source_branch,
            "commit_hash": deployment.commit_hash,
            "commit_message": deployment.commit_message,
        },
        "triggered_by": triggered_by,
        "failure_code": deployment.failure_code,
        "failure_message": deployment.failure_message,
        "serve_failure_code": deployment.serve_failure_code,
        "serve_failure_message": deployment.serve_failure_message,
        "duration_seconds": duration_seconds,
        "claimed_at": ts(deployment.claimed_at),
        "build_started_at": ts(deployment.build_started_at),
        "build_finished_at": ts(deployment.build_finished_at),
        "serve_started_at": ts(deployment.serve_started_at),
        "serve_finished_at": ts(deployment.serve_finished_at),
        "created_at": ts(deployment.created_at),
    });
    if let serde_json::Value::Object(map) =
        urls.urls(deployment, effective_preview_ids.contains(&deployment.id))
        && let serde_json::Value::Object(view_map) = &mut view
    {
        view_map.extend(map);
    }
    view
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
    access: &super::ProjectAccess,
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

// --- Create -----------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateDeploymentRequest {
    #[serde(default = "default_environment")]
    pub environment: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub commit_hash: Option<String>,
    #[serde(default)]
    pub commit_message: Option<String>,
    #[serde(default)]
    pub serve_node_id: Option<Uuid>,
}

fn default_environment() -> String {
    "production".to_owned()
}

fn map_schedule_error(error: ScheduleError, op: &'static str) -> AppError {
    match error {
        ScheduleError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        error @ ScheduleError::InvalidData => AppError::Infrastructure {
            op,
            source: anyhow::Error::new(error),
        },
        other => AppError::Conflict {
            op,
            message: other.to_string(),
        },
    }
}

pub(crate) fn map_delivery_error(error: DeliveryError, op: &'static str) -> AppError {
    match error {
        DeliveryError::ReleaseAlreadyPending => AppError::Conflict {
            op,
            message: error.to_string(),
        },
        DeliveryError::State(error) => map_state_error(error, op),
        DeliveryError::Schedule(error) => map_schedule_error(error, op),
        DeliveryError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        DeliveryError::InvalidResources
        | DeliveryError::InvalidUnsuccessfulBuildTransition
        | DeliveryError::Other(_) => AppError::Infrastructure {
            op,
            source: anyhow::Error::new(error),
        },
    }
}

async fn create_placed_deployment(
    db: &sea_orm::DatabaseConnection,
    params: CreateDeploymentParams,
    selected_node_id: Option<Uuid>,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let requested = deployments::runtime_serve_resources(&params.project.runtime);
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let placement =
        match scheduler::place_deployment(&transaction, requested, selected_node_id).await {
            Ok(placement) => placement,
            Err(error) => {
                let error = map_schedule_error(error, op);
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };
    let deployment = match deployments::create_deployment(&transaction, params, placement).await {
        Ok(deployment) => deployment,
        Err(source) => {
            let _ = transaction.rollback().await;
            return Err(AppError::Infrastructure { op, source });
        }
    };
    let mode = match placement.mode {
        PlacementMode::Automatic => "automatic",
        PlacementMode::Manual => "manual",
    };
    if let Err(source) = deployments::append_event(
        &transaction,
        deployment.id,
        crate::infra::database::entity::DeploymentEventKind::Serve,
        "deployment assigned to serve node",
        json!({
            "mode": mode,
            "serve_node_id": placement.node_id,
            "resources": requested,
            "overcommitted": placement.overcommitted,
        }),
    )
    .await
    {
        let _ = transaction.rollback().await;
        return Err(AppError::Infrastructure { op, source });
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    Ok(deployment)
}

/// POST /api/v1/projects/{project_id}/deployments
pub async fn create(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateDeploymentRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.create";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;

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
            branch: super::optional_trimmed(body.branch),
            commit_hash: super::optional_trimmed(body.commit_hash),
            commit_message: super::optional_trimmed(body.commit_message),
            preview_host,
            source_credential_version_id,
        },
        body.serve_node_id,
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

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(access.team.id),
            action: "deployment.created".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "project_id": access.project.id,
                "environment": deployments::environment_value(&deployment.environment),
            }),
        },
    )
    .await;

    // Non-static runtimes are reserved but not implemented: fail the
    // deployment immediately with the stable message instead of letting it
    // sit in the queue forever.
    let deployment = match deployments::runtime_failure(&deployment.runtime_kind) {
        Some((code, message)) => {
            let transaction = db
                .begin()
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

    Ok(ok_response(json!({
        "deployment": deployment_view(&deployment, &urls, &preview_ids, &users, &nodes),
    })))
}

async fn preview_host_for_project(
    db: &sea_orm::DatabaseConnection,
    project: &crate::infra::database::entity::project::Model,
) -> Option<String> {
    let sources = hosts::list_sources(db).await.ok()?;
    match hosts::select_auto_assign_source(&sources) {
        hosts::AutoAssignSelection::Source(source) => {
            // The host embeds the deployment id, which is generated inside
            // create_deployment; pre-generate one here and thread it through
            // instead would complicate creation, so derive from a fresh UUID
            // and store it directly on the deployment row at creation time.
            Some(hosts::preview_host_for(
                &project.slug,
                Uuid::now_v7(),
                &source.base_domain,
            ))
        }
        _ => None,
    }
}

fn environment_gets_preview_host(environment: &DeploymentEnvironment) -> bool {
    matches!(
        environment,
        DeploymentEnvironment::Production | DeploymentEnvironment::Preview
    )
}

// --- List / detail ----------------------------------------------------------

#[derive(Deserialize)]
pub struct ListDeploymentsQuery {
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub build_status: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub offset: Option<u64>,
}

/// GET /api/v1/projects/{project_id}/deployments
pub async fn list(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListDeploymentsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.list";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;

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

    Ok(ok_response(json!({
        "deployments": deployments
            .iter()
            .map(|deployment| deployment_view(deployment, &urls, &preview_ids, &users, &nodes))
            .collect::<Vec<_>>(),
    })))
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

/// GET /api/v1/projects/{project_id}/serve-nodes
pub async fn serve_nodes(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.serve_nodes";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;
    let requested = deployments::runtime_serve_resources(&access.project.runtime);
    let candidates = scheduler::eligible_candidates(db)
        .await
        .map_err(|error| map_schedule_error(error, OP))?;
    let node_ids: Vec<Uuid> = candidates
        .iter()
        .map(|candidate| candidate.node_id)
        .collect();
    let nodes: HashMap<Uuid, node::Model> = if node_ids.is_empty() {
        HashMap::new()
    } else {
        node::Entity::find()
            .filter(node::Column::Id.is_in(node_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|node| (node.id, node))
            .collect()
    };

    let views = candidates
        .iter()
        .filter_map(|candidate| {
            let node = nodes.get(&candidate.node_id)?;
            let placement = scheduler::choose_candidate(
                std::slice::from_ref(candidate),
                requested,
                None,
            )
            .ok();
            let max_deployments = u64::from(candidate.capacity.max_deployments);
            let overflow_used = candidate.usage.deployments.saturating_sub(max_deployments);
            Some(json!({
                "id": node.id,
                "name": node.name,
                "healthy": true,
                "capacity": candidate.capacity,
                "usage": candidate.usage,
                "normal_available": placement.is_some_and(|placement| !placement.overcommitted),
                "schedulable": placement.is_some(),
                "overflow_only": placement.is_some_and(|placement| placement.overcommitted),
                "disk_available_mb": candidate.capacity.disk_mb.saturating_sub(candidate.usage.disk_mb),
                "remaining_overflow_slots": 2_u64.saturating_sub(overflow_used),
            }))
        })
        .collect::<Vec<_>>();

    Ok(ok_response(json!({ "serve_nodes": views })))
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.detail";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;
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
    let policy = deployments::review_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;

    Ok(ok_response(json!({
        "deployment": deployment_view(&deployment, &urls, &preview_ids, &users, &nodes),
        "events": events.iter().map(|event| json!({
            "id": event.id,
            "kind": event_kind_value(&event.kind),
            "message": event.message,
            "metadata": event.metadata,
            "created_at": ts(event.created_at),
        })).collect::<Vec<_>>(),
        "artifacts": artifacts.iter().map(|artifact| json!({
            "id": artifact.id,
            "kind": artifact_kind_value(&artifact.kind),
            "storage_path": artifact.storage_path,
            "checksum_sha256": artifact.checksum_sha256,
            "size_bytes": artifact.size_bytes,
            "manifest": artifact.manifest,
            "created_at": ts(artifact.created_at),
        })).collect::<Vec<_>>(),
        "reviews": reviews.iter().map(|review| json!({
            "id": review.id,
            "status": review_status_value(&review.status),
            "reviewer_user_id": review.reviewer_user_id,
            "reason": review.reason,
            "requested_at": ts(review.requested_at),
            "reviewed_at": ts(review.reviewed_at),
        })).collect::<Vec<_>>(),
        "review_required": matches!(policy.mode_for(&deployment.environment), ReviewMode::Manual),
        "was_active": was_active,
    })))
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

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/events
pub async fn events(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.events";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;

    let events = deployments::list_events(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({
        "events": events.iter().map(|event| json!({
            "id": event.id,
            "kind": event_kind_value(&event.kind),
            "message": event.message,
            "metadata": event.metadata,
            "created_at": ts(event.created_at),
        })).collect::<Vec<_>>(),
    })))
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/artifacts
pub async fn artifacts(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.artifacts";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let db = super::database(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;

    let artifacts = deployments::list_artifacts(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(json!({
        "artifacts": artifacts.iter().map(|artifact| json!({
            "id": artifact.id,
            "kind": artifact_kind_value(&artifact.kind),
            "storage_path": artifact.storage_path,
            "checksum_sha256": artifact.checksum_sha256,
            "size_bytes": artifact.size_bytes,
            "manifest": artifact.manifest,
            "created_at": ts(artifact.created_at),
        })).collect::<Vec<_>>(),
    })))
}

// --- Operations -------------------------------------------------------------

/// Cancels a deployment: validates the transition, sets the cooperative
/// cancel flag for the building Node, releases the concurrency slot, and
/// records the audit event. Shared by the REST handler and the websocket
/// cancel path.
pub(crate) async fn cancel_deployment_core(
    db: &sea_orm::DatabaseConnection,
    cache: &grass_cache::CacheStore,
    deployment: deployment::Model,
    actor_user_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let was_running = cancellation_releases_build_slot(&deployment.build_status);
    let team_id = deployment.team_id;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let deployment = delivery::transition_unsuccessful_build(
        &transaction,
        deployment,
        BuildTransition {
            to: DeploymentBuildStatus::Canceled,
            stage: None,
            failure_code: None,
            failure_message: Some("canceled by user".to_owned()),
            build_node_id: None,
        },
    )
    .await
    .map_err(|error| map_delivery_error(error, op))?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;

    if was_running {
        // Cooperative flag for the Node driving this build; it stops the
        // container and releases the concurrency slot when it sees it.
        use grass_cache::Cache;
        let _ = cache
            .set(
                &cancel_flag_key(deployment.id),
                "1",
                std::time::Duration::from_secs(60 * 60 * 24),
            )
            .await;
        QuotaService::new(db, cache)
            .release_build_slot_once(team_id, deployment.id)
            .await;
    }

    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(team_id),
            action: "deployment.canceled".to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({}),
        },
    )
    .await;

    Ok(deployment)
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/cancel
pub async fn cancel(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.cancel";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;

    let deployment =
        cancel_deployment_core(db, cache, deployment, session.data.user_id, OP).await?;

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;
    Ok(ok_response(json!({
        "deployment": deployment_view(
            &deployment,
            &urls,
            &preview_ids,
            &HashMap::new(),
            &nodes,
        ),
    })))
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/retry
pub async fn retry(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.retry";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_member(OP)?;
    let db = super::database(&state, OP)?;
    let cache = super::cache(&state, OP)?;
    let source_deployment = load_deployment(db, &access, deployment_id, OP).await?;

    if !matches!(
        source_deployment.build_status,
        DeploymentBuildStatus::Failed | DeploymentBuildStatus::Canceled
    ) {
        return Err(AppError::Conflict {
            op: OP,
            message: "only failed or canceled deployments can be retried".to_owned(),
        });
    }
    projects::ensure_deployable(&access.project).map_err(|error| AppError::Conflict {
        op: OP,
        message: error.to_string(),
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

    let preview_host = if environment_gets_preview_host(&source_deployment.environment) {
        preview_host_for_project(db, &access.project).await
    } else {
        None
    };

    let new_deployment = match create_placed_deployment(
        db,
        CreateDeploymentParams {
            project: access.project.clone(),
            environment: source_deployment.environment.clone(),
            triggered_by_user_id: Some(session.data.user_id),
            branch: source_deployment.source_branch.clone(),
            commit_hash: source_deployment.commit_hash.clone(),
            commit_message: source_deployment.commit_message.clone(),
            preview_host,
            source_credential_version_id: source_deployment.source_credential_version_id,
        },
        None,
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
        .commit(OP, reservation, "deployment", Some(new_deployment.id))
        .await?;

    deployments::append_event(
        db,
        new_deployment.id,
        crate::infra::database::entity::DeploymentEventKind::System,
        "retry of earlier deployment",
        json!({ "retried_from": source_deployment.id }),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    let new_deployment = match deployments::runtime_failure(&new_deployment.runtime_kind) {
        Some((code, message)) => {
            let transaction = db
                .begin()
                .await
                .map_err(|source| AppError::Infrastructure {
                    op: OP,
                    source: source.into(),
                })?;
            let deployment = delivery::transition_unsuccessful_build(
                &transaction,
                new_deployment,
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
        None => new_deployment,
    };

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&new_deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, OP).await?;
    Ok(ok_response(json!({
        "deployment": deployment_view(
            &new_deployment,
            &urls,
            &preview_ids,
            &HashMap::new(),
            &nodes,
        ),
    })))
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/promote
pub async fn promote(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    activate_deployment(
        state,
        session,
        project_id,
        deployment_id,
        ReleaseReason::Promote,
        "deployments.promote",
    )
    .await
}

/// POST /api/v1/projects/{project_id}/deployments/{deployment_id}/rollback
///
/// Rolls the environment back to this deployment. The target must be a
/// previously active deployment with a ready build.
pub async fn rollback(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.rollback";
    let db = super::database(&state, OP)?;
    let was_active = deployments::was_active(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if !was_active {
        return Err(AppError::Conflict {
            op: OP,
            message: "only previously active deployments can be rollback targets".to_owned(),
        });
    }

    activate_deployment(
        state,
        session,
        project_id,
        deployment_id,
        ReleaseReason::Rollback,
        OP,
    )
    .await
}

async fn activate_deployment(
    state: ControlApiState,
    session: Session,
    project_id: Uuid,
    deployment_id: Uuid,
    reason: ReleaseReason,
    op: &'static str,
) -> Result<axum::response::Response, AppError> {
    let access = super::project_access(&state, &session, project_id, false, op).await?;
    access.require_admin(op)?;
    let db = super::database(&state, op)?;
    let deployment = load_deployment(db, &access, deployment_id, op).await?;

    if !matches!(deployment.build_status, DeploymentBuildStatus::Ready) {
        return Err(AppError::Conflict {
            op,
            message: "only deployments with a ready build can be activated".to_owned(),
        });
    }
    if matches!(deployment.release_status, DeploymentReleaseStatus::Active) {
        return Err(AppError::Conflict {
            op,
            message: "deployment is already active".to_owned(),
        });
    }

    // Production activation must pass the review policy; rejected builds
    // can never activate.
    let policy = deployments::review_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let review_required = matches!(policy.mode_for(&deployment.environment), ReviewMode::Manual);
    match deployment.release_status {
        DeploymentReleaseStatus::Rejected => {
            return Err(AppError::Conflict {
                op,
                message: "rejected deployments must be re-submitted for review".to_owned(),
            });
        }
        DeploymentReleaseStatus::PendingReview => {
            return Err(AppError::Conflict {
                op,
                message: "deployment is waiting for review".to_owned(),
            });
        }
        DeploymentReleaseStatus::Draft if review_required => {
            return Err(AppError::Conflict {
                op,
                message: "release review is required before activation".to_owned(),
            });
        }
        _ => {}
    }

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let outcome = delivery::request_release(
        &transaction,
        deployment,
        reason.clone(),
        session.data.user_id,
        AuditEventVisibility::Team,
    )
    .await
    .map_err(|error| map_delivery_error(error, op))?;
    let (deployment, release_pending) = match outcome {
        ReleaseRequestOutcome::Activated(deployment) => (deployment, false),
        ReleaseRequestOutcome::SyncQueued(deployment) => (deployment, true),
    };
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: Some(access.team.id),
            action: delivery::release_audit_action(&reason, release_pending).to_owned(),
            target_type: "deployment".to_owned(),
            target_id: Some(deployment.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "project_id": project_id,
                "release_pending": release_pending,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;

    let urls = UrlContext::load(db, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let nodes = load_nodes(db, std::slice::from_ref(&deployment))
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    let preview_ids = effective_preview_ids(db, access.project.id, op).await?;
    let response = json!({
        "deployment": deployment_view(
            &deployment,
            &urls,
            &preview_ids,
            &HashMap::new(),
            &nodes,
        ),
        "release_pending": release_pending,
    });
    Ok(if release_pending {
        accepted_response(response).into_response()
    } else {
        ok_response(response).into_response()
    })
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
    fn every_environment_gets_a_protected_preview_host() {
        assert!(environment_gets_preview_host(
            &DeploymentEnvironment::Preview
        ));
        assert!(environment_gets_preview_host(
            &DeploymentEnvironment::Production
        ));
    }

    #[test]
    fn static_and_ssr_requests_use_fixed_first_phase_resources() {
        assert_eq!(
            deployments::runtime_serve_resources(
                &crate::infra::database::entity::ProjectRuntime::Static
            ),
            grass_node_protocol::ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 256,
            }
        );
        assert_eq!(
            deployments::runtime_serve_resources(
                &crate::infra::database::entity::ProjectRuntime::Ssr
            ),
            grass_node_protocol::ServeResources {
                cpu_millicores: 200,
                memory_mb: 256,
                disk_mb: 512,
            }
        );
    }

    #[test]
    fn user_cancel_owns_the_single_slot_release() {
        for status in [
            DeploymentBuildStatus::Claimed,
            DeploymentBuildStatus::Queued,
            DeploymentBuildStatus::Building,
        ] {
            assert!(cancellation_releases_build_slot(&status));
        }

        // After cancel_deployment_core transitions the row, every later Node
        // report takes the early cancellation branch and cannot reach the
        // terminal-stage slot release.
        assert!(cancellation_was_requested(
            &DeploymentBuildStatus::Canceled,
            false
        ));
        assert!(cancellation_was_requested(
            &DeploymentBuildStatus::Building,
            true
        ));
    }

    #[test]
    fn canceling_a_non_running_deployment_does_not_release_a_slot() {
        for status in [
            DeploymentBuildStatus::Pending,
            DeploymentBuildStatus::Ready,
            DeploymentBuildStatus::Failed,
            DeploymentBuildStatus::Canceled,
        ] {
            assert!(!cancellation_releases_build_slot(&status));
        }
    }
}
