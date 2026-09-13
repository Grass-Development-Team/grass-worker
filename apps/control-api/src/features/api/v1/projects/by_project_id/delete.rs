use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::{
        project_lifecycle::{
            finalize_deleted_project_resources, record_lifecycle_event,
            release_deleted_project_quota, soft_delete_project_records,
        },
        projects,
    },
    infra::{
        database::entity::project,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/projects/{project_id}/delete", axum::routing::post(delete))
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

/// POST /api/v1/projects/{project_id}/delete — soft delete.
pub async fn delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.delete";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::IncludingDeleted,
        OP,
    )
    .await?;
    access.require_admin(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let cache = crate::infra::http::cache(&state, OP)?;

    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deletion = soft_delete_project_records(&transaction, access.project)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    if deletion.newly_deleted {
        record_lifecycle_event(
            &transaction,
            session.data.user_id,
            "project.deleted",
            &deletion.project,
            "/projects".to_owned(),
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    let project = deletion.project;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let warnings = if deletion.newly_deleted {
        let platform_secret = state.config.read().unwrap().secrets.secret_key.clone();
        finalize_deleted_project_resources(
            db,
            cache,
            &platform_secret,
            OP,
            &project,
            &deletion.bindings,
        )
        .await?
    } else {
        release_deleted_project_quota(db, cache, OP, &project, &deletion.bindings).await?;
        Vec::new()
    };
    Ok(ok_response(DeleteResponse {
        project: project_view(&project),
        warnings,
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
struct DeleteResponse {
    project: ProjectResponse,
    warnings: Vec<crate::domain::project_lifecycle::ProjectCleanupWarning>,
}
