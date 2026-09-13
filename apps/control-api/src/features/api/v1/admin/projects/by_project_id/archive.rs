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
        "/projects/{project_id}/archive",
        axum::routing::post(archive),
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

/// POST /api/v1/admin/projects/{project_id}/archive
pub async fn archive(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let project = crate::domain::admin_projects::archive(&state, data.user_id, project_id).await?;
    Ok(ok_response(ArchiveResponse {
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
struct ArchiveResponse {
    project: ProjectResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::project;
    use time::OffsetDateTime;
    use uuid::Uuid;
    #[test]
    fn project_status_and_view_expose_deleted_rows() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let project = project::Model {
            id: Uuid::now_v7(),
            team_id: Uuid::now_v7(),
            created_by_user_id: None,
            slug: "deleted-project".to_owned(),
            name: "Deleted project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: None,
            deleted_at: Some(now),
            created_at: now,
            updated_at: now,
        };

        assert_eq!(project_status(&project), "deleted");
        let view = serde_json::to_value(project_view(&project, None, None)).unwrap();
        assert_eq!(view["status"], "deleted");
        assert_eq!(
            view["deleted_at"],
            serde_json::json!("1970-01-01T00:00:00Z")
        );
    }
}
