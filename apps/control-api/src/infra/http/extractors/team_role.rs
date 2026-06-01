use std::collections::HashMap;

use axum::{
    extract::{FromRequestParts, Path},
    http::request::Parts,
};
use uuid::Uuid;

use crate::{
    domain::teams,
    infra::{database::entity::TeamMemberRole, error::AppError},
    state::ControlApiState,
};

use super::Session;

pub struct TeamRole {
    pub team_id: Uuid,
    pub user_id: Uuid,
    pub role: TeamMemberRole,
}

impl TeamRole {
    pub fn require_owner(&self, op: &'static str) -> Result<(), AppError> {
        if self.role != TeamMemberRole::Owner {
            return Err(AppError::Forbidden {
                op,
                message: "owner role required".to_owned(),
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
}

impl FromRequestParts<ControlApiState> for TeamRole {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ControlApiState,
    ) -> Result<Self, Self::Rejection> {
        let Path(params) = Path::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|_| AppError::Validation {
                op: "team.path_params",
                message: "invalid team path parameters".to_owned(),
            })?;

        let team_id = params
            .get("team_id")
            .ok_or_else(|| AppError::Validation {
                op: "team.path_params.missing_team_id",
                message: "team_id path parameter is required".to_owned(),
            })?
            .parse::<Uuid>()
            .map_err(|_| AppError::Validation {
                op: "team.path_params.invalid_team_id",
                message: "invalid team_id path parameter".to_owned(),
            })?;

        let session = Session::from_request_parts(parts, state).await?;
        let db = state.try_database().ok_or_else(|| AppError::Internal {
            op: "team.role.no_database",
            message: "database not available".to_owned(),
        })?;

        let role = teams::member_role(db, team_id, session.data.user_id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "team.role.lookup",
                source,
            })?
            .ok_or_else(|| AppError::Forbidden {
                op: "team.role.not_member",
                message: "not a member of this team".to_owned(),
            })?;

        Ok(Self {
            team_id,
            user_id: session.data.user_id,
            role,
        })
    }
}
