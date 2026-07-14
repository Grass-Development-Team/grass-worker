pub mod create;
pub mod detail;
pub mod invitations;
pub mod list;
pub mod members;
pub mod update;

use axum::{
    Router,
    routing::{get, patch, post},
};
use sea_orm::DatabaseConnection;

use crate::{infra::error::AppError, state::ControlApiState};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/teams", get(list::handler).post(create::handler))
        .route(
            "/teams/{team_id}",
            get(detail::handler).patch(update::handler),
        )
        .route("/teams/{team_id}/members", get(members::list))
        .route(
            "/teams/{team_id}/members/{user_id}",
            patch(members::update_role).delete(members::remove),
        )
        .route("/team-invitations/accept", post(invitations::accept))
        .route("/teams/{team_id}/invitations", post(invitations::create))
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

pub(crate) fn validate_required(value: &str, op: &'static str, name: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::Validation {
            op,
            message: format!("{name} is required"),
        });
    }
    Ok(())
}

pub(crate) fn normalize_slug(value: &str, op: &'static str) -> Result<String, AppError> {
    grass_validator::normalize_slug(value).map_err(|error| AppError::Validation {
        op,
        message: error.to_string(),
    })
}

pub(crate) fn map_team_write_error(source: anyhow::Error, op: &'static str) -> AppError {
    if crate::infra::database::is_unique_violation(&source) {
        AppError::Conflict {
            op,
            message: "team slug is already in use".to_owned(),
        }
    } else {
        AppError::Infrastructure { op, source }
    }
}

pub(crate) fn parse_role(
    role: &str,
    op: &'static str,
) -> Result<crate::infra::database::entity::TeamMemberRole, AppError> {
    use crate::infra::database::entity::TeamMemberRole;

    match role.trim().to_lowercase().as_str() {
        "owner" => Ok(TeamMemberRole::Owner),
        "admin" => Ok(TeamMemberRole::Admin),
        "member" => Ok(TeamMemberRole::Member),
        "viewer" => Ok(TeamMemberRole::Viewer),
        _ => Err(AppError::Validation {
            op,
            message: format!("invalid role: {role}"),
        }),
    }
}

pub(crate) fn role_value(role: &crate::infra::database::entity::TeamMemberRole) -> &'static str {
    use crate::infra::database::entity::TeamMemberRole;

    match role {
        TeamMemberRole::Owner => "owner",
        TeamMemberRole::Admin => "admin",
        TeamMemberRole::Member => "member",
        TeamMemberRole::Viewer => "viewer",
    }
}

pub(crate) fn kind_value(kind: &crate::infra::database::entity::TeamKind) -> &'static str {
    use crate::infra::database::entity::TeamKind;

    match kind {
        TeamKind::Personal => "personal",
        TeamKind::Team => "team",
    }
}
