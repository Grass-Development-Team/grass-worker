use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{deployments, projects},
    infra::{
        database::entity::{deployment, project, team},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/unarchive",
        axum::routing::post(unarchive),
    )
}

fn deployment_summary(deployment: &deployment::Model) -> DeploymentSummaryResponse {
    DeploymentSummaryResponse {
        id: deployment.id,
        environment: deployments::environment_value(&deployment.environment),
        build_status: deployments::build_status_value(&deployment.build_status),
        release_status: deployments::release_status_value(&deployment.release_status),
        created_at: deployment.created_at,
    }
}

fn project_view(
    project: &project::Model,
    team: Option<&team::Model>,
    latest: Option<&deployment::Model>,
) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        team: team.map(|team| ProjectTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
        }),
        latest_deployment: latest.map(deployment_summary),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        status: project_status(project),
        created_at: project.created_at,
    }
}

fn project_status(project: &project::Model) -> &'static str {
    if project.deleted_at.is_some() {
        "deleted"
    } else if project.archived_at.is_some() {
        "archived"
    } else {
        "active"
    }
}

/// POST /api/v1/admin/projects/{project_id}/unarchive
async fn unarchive(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let project =
        crate::domain::admin_projects::unarchive(&state, data.user_id, project_id).await?;
    Ok(ok_response(UnarchiveResponse {
        project: project_view(&project, None, None),
    }))
}

#[derive(serde::Serialize)]
struct DeploymentSummaryResponse {
    id: uuid::Uuid,
    environment: &'static str,
    build_status: &'static str,
    release_status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ProjectTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    team: Option<ProjectTeamResponse>,
    latest_deployment: Option<DeploymentSummaryResponse>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UnarchiveResponse {
    project: ProjectResponse,
}
