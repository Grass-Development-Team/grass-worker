pub mod create;
pub mod detail;
pub mod list;
pub mod update;

use axum::{Router, routing::get};
use sea_orm::DatabaseConnection;

use crate::{infra::error::AppError, state::ControlApiState};

pub fn router() -> Router<ControlApiState> {
    Router::new()
        .route("/teams", get(list::handler).post(create::handler))
        .route(
            "/teams/{team_id}",
            get(detail::handler).patch(update::handler),
        )
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

pub(crate) fn kind_value(kind: &crate::infra::database::entity::TeamKind) -> &'static str {
    use crate::infra::database::entity::TeamKind;

    match kind {
        TeamKind::Personal => "personal",
        TeamKind::Team => "team",
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
