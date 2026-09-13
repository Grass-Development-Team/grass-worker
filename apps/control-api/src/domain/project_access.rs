use uuid::Uuid;

use crate::{
    domain::{projects, teams},
    infra::{
        database::entity::{TeamMemberRole, project, team},
        error::AppError,
        http::database,
    },
    state::ControlApiState,
};

#[derive(Clone, Copy)]
pub(crate) enum ProjectScope {
    Active,
    IncludingDeleted,
}

pub(crate) struct ProjectAccess {
    pub project: project::Model,
    pub team: team::Model,
    pub role: TeamMemberRole,
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
        if !role_can_admin(&self.role) {
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

fn role_can_admin(role: &TeamMemberRole) -> bool {
    matches!(role, TeamMemberRole::Owner | TeamMemberRole::Admin)
}

/// Loads a project and authorizes the session user through the owning
/// team's membership. `ProjectScope::IncludingDeleted` is used by restore and hard delete.
pub(crate) async fn load(
    state: &ControlApiState,
    actor_user_id: Uuid,
    project_id: Uuid,
    scope: ProjectScope,
    op: &'static str,
) -> Result<ProjectAccess, AppError> {
    let db = database(state, op)?;
    let project = if matches!(scope, ProjectScope::IncludingDeleted) {
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

    let role = teams::member_role(db, project.team_id, actor_user_id)
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_owner_and_admin_can_manage_project_source_credentials() {
        assert!(role_can_admin(&TeamMemberRole::Owner));
        assert!(role_can_admin(&TeamMemberRole::Admin));
        assert!(!role_can_admin(&TeamMemberRole::Member));
        assert!(!role_can_admin(&TeamMemberRole::Viewer));
    }
}
