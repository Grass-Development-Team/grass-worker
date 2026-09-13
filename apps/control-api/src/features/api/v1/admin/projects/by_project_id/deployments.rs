use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::{
    domain::{deployments, projects},
    infra::{
        database::entity::{deployment, project},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments",
        axum::routing::get(deployments),
    )
}

fn admin_deployment_view(item: &deployment::Model) -> DeploymentResponse {
    DeploymentResponse {
        id: item.id,
        project_id: item.project_id,
        environment: deployments::environment_value(&item.environment),
        build_status: deployments::build_status_value(&item.build_status),
        serve_status: deployments::serve_status_value(&item.serve_status),
        release_status: deployments::release_status_value(&item.release_status),
        release_pending: item.pending_release_reason.is_some(),
        preview_host: item.preview_host.clone(),
        source_repository_url: item.source_repository_url.clone(),
        source_branch: item.source_branch.clone(),
        commit_hash: item.commit_hash.clone(),
        commit_message: item.commit_message.clone(),
        build_stage: item.build_stage.clone(),
        failure_code: item.failure_code.clone(),
        failure_message: item.failure_message.clone(),
        serve_failure_code: item.serve_failure_code.clone(),
        serve_failure_message: item.serve_failure_message.clone(),
        claimed_at: item.claimed_at,
        build_started_at: item.build_started_at,
        build_finished_at: item.build_finished_at,
        serve_started_at: item.serve_started_at,
        serve_finished_at: item.serve_finished_at,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}

/// GET /api/v1/admin/projects/{project_id}/deployments
async fn deployments(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.deployments";
    let db = crate::infra::http::database(&state, OP)?;
    let _project = load_project_any(db, project_id, OP).await?;
    let items = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_desc(deployment::Column::CreatedAt)
        .order_by_desc(deployment::Column::Id)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(DeploymentsResponse {
        deployments: items.iter().map(admin_deployment_view).collect::<Vec<_>>(),
    }))
}

async fn load_project_any<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    op: &'static str,
) -> Result<project::Model, AppError> {
    projects::get_by_id_any(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })
}

#[derive(serde::Serialize)]
struct DeploymentResponse {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    environment: &'static str,
    build_status: &'static str,
    serve_status: &'static str,
    release_status: &'static str,
    release_pending: bool,
    preview_host: Option<String>,
    source_repository_url: Option<String>,
    source_branch: Option<String>,
    commit_hash: Option<String>,
    commit_message: Option<String>,
    build_stage: Option<String>,
    failure_code: Option<String>,
    failure_message: Option<String>,
    serve_failure_code: Option<String>,
    serve_failure_message: Option<String>,
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
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DeploymentsResponse {
    deployments: Vec<DeploymentResponse>,
}
