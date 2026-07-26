pub mod create;
pub mod deployments;
pub mod detail;
pub mod hosts;
pub mod lifecycle;
pub mod list;

use axum::{
    Router,
    routing::{get, post},
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::{
    domain::{projects, teams},
    infra::{
        database::entity::{TeamMemberRole, project, team},
        error::AppError,
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/projects", get(list::handler).post(create::handler))
        .route(
            "/projects/{project_id}",
            get(detail::get).patch(detail::update),
        )
        .route("/projects/{project_id}/archive", post(lifecycle::archive))
        .route(
            "/projects/{project_id}/unarchive",
            post(lifecycle::unarchive),
        )
        .route("/projects/{project_id}/delete", post(lifecycle::delete))
        .route("/projects/{project_id}/restore", post(lifecycle::restore))
        .route(
            "/projects/{project_id}/transfer-team",
            post(lifecycle::transfer_team),
        )
        .route(
            "/projects/{project_id}/hard-delete",
            post(lifecycle::hard_delete),
        )
        .route(
            "/projects/{project_id}/hosts",
            get(hosts::list).post(hosts::create),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}",
            axum::routing::patch(hosts::update).delete(hosts::remove),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/primary",
            post(hosts::set_primary),
        )
        .route(
            "/projects/{project_id}/hosts/{host_id}/provision",
            post(hosts::provision),
        )
        .route(
            "/projects/{project_id}/deployments",
            get(deployments::list).post(deployments::create),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}",
            get(deployments::detail),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/events",
            get(deployments::events),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/artifacts",
            get(deployments::artifacts),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/cancel",
            post(deployments::cancel),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/retry",
            post(deployments::retry),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/promote",
            post(deployments::promote),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/rollback",
            post(deployments::rollback),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/review/request",
            post(deployments::review::request),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/review/approve",
            post(deployments::review::approve),
        )
        .route(
            "/projects/{project_id}/deployments/{deployment_id}/review/reject",
            post(deployments::review::reject),
        )
}

pub(crate) struct ProjectAccess {
    pub project: project::Model,
    pub team: team::Model,
    pub role: TeamMemberRole,
    #[allow(dead_code)] // Read by deployment slices in Milestone 5.
    pub user_id: Uuid,
}

impl ProjectAccess {
    pub fn require_member(&self, op: &'static str) -> Result<(), AppError> {
        if matches!(self.role, TeamMemberRole::Viewer) {
            return Err(AppError::Forbidden {
                op,
                message: "member role required".to_owned(),
            });
        }
        Ok(())
    }

    pub fn require_admin(&self, op: &'static str) -> Result<(), AppError> {
        if !matches!(self.role, TeamMemberRole::Owner | TeamMemberRole::Admin) {
            return Err(AppError::Forbidden {
                op,
                message: "admin role required".to_owned(),
            });
        }
        Ok(())
    }

    pub fn require_owner(&self, op: &'static str) -> Result<(), AppError> {
        if !matches!(self.role, TeamMemberRole::Owner) {
            return Err(AppError::Forbidden {
                op,
                message: "owner role required".to_owned(),
            });
        }
        Ok(())
    }
}

/// Loads a project and authorizes the session user through the owning
/// team's membership. `include_deleted` is used by restore and hard delete.
pub(crate) async fn project_access(
    state: &ControlApiState,
    session: &Session,
    project_id: Uuid,
    include_deleted: bool,
    op: &'static str,
) -> Result<ProjectAccess, AppError> {
    let db = database(state, op)?;
    let project = if include_deleted {
        projects::get_by_id_any(db, project_id).await
    } else {
        projects::get_by_id(db, project_id).await
    }
    .map_err(|source| AppError::Infrastructure { op, source })?
    .ok_or_else(|| AppError::NotFound {
        op,
        message: "project not found".to_owned(),
    })?;

    let team = teams::get_by_id(db, project.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "team not found".to_owned(),
        })?;

    let role = teams::member_role(db, project.team_id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::Forbidden {
            op,
            message: "not a member of this team".to_owned(),
        })?;

    Ok(ProjectAccess {
        project,
        team,
        role,
        user_id: session.data.user_id,
    })
}

pub(crate) fn database<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a DatabaseConnection, AppError> {
    state.try_database().ok_or_else(|| AppError::Internal {
        op,
        message: "database not available".to_owned(),
    })
}

pub(crate) fn cache<'a>(
    state: &'a ControlApiState,
    op: &'static str,
) -> Result<&'a grass_cache::CacheStore, AppError> {
    state.try_cache().ok_or_else(|| AppError::Internal {
        op,
        message: "cache not available".to_owned(),
    })
}

pub(crate) fn project_view(project: &project::Model) -> serde_json::Value {
    serde_json::json!({
        "id": project.id,
        "team_id": project.team_id,
        "slug": project.slug,
        "name": project.name,
        "runtime": projects::runtime_value(&project.runtime),
        "repository_url": project.repository_url,
        "default_branch": project.default_branch,
        "install_command": project.install_command,
        "build_command": project.build_command,
        "output_directory": project.output_directory,
        "source_config": project.source_config,
        "build_config": project.build_config,
        "archived_at": project.archived_at,
        "deleted_at": project.deleted_at,
        "created_at": project.created_at,
        "updated_at": project.updated_at,
    })
}

pub(crate) fn optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim().to_owned();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}
