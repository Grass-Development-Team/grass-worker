use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{project_lifecycle::record_lifecycle_event, projects},
    infra::{
        database::entity::project,
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

fn project_view(project: &project::Model) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        team_id: project.team_id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        default_branch: project.default_branch.clone(),
        install_command: project.install_command.clone(),
        build_command: project.build_command.clone(),
        output_directory: project.output_directory.clone(),
        source_config: project.source_config.clone(),
        build_config: project.build_config.clone(),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        created_at: project.created_at,
        updated_at: project.updated_at,
    }
}

/// POST /api/v1/projects/{project_id}/unarchive
async fn unarchive(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.unarchive";
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
    let project = projects::set_archived(&transaction, access.project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_lifecycle_event(
        &transaction,
        session.data.user_id,
        "project.unarchived",
        &project,
        format!("/projects/{}", project.id),
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

    Ok(ok_response(UnarchiveResponse {
        project: project_view(&project),
    }))
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct UnarchiveResponse {
    project: ProjectResponse,
}
