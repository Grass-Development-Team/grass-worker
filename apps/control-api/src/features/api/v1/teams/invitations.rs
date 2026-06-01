use axum::{Json, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::teams::{self, CreateInvitationParams},
    infra::{
        database::entity::TeamMemberRole,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct CreateInvitationRequest {
    pub email: String,
    pub role: String,
}

pub async fn create(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Json(body): Json<CreateInvitationRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.invitations.create.admin_required")?;

    if body.email.trim().is_empty() || !body.email.contains('@') {
        return Err(AppError::Validation {
            op: "teams.invitations.create.invalid_email",
            message: "invalid email address".to_owned(),
        });
    }

    let role = super::parse_role(&body.role, "teams.invitations.create.invalid_role")?;
    if matches!(role, TeamMemberRole::Owner) {
        team_role.require_owner("teams.invitations.create.owner_required")?;
    }

    let db = super::database(&state, "teams.invitations.create.no_database")?;
    let invitation = teams::create_invitation(
        db,
        CreateInvitationParams {
            team_id: team_role.team_id,
            email: body.email.trim().to_lowercase(),
            role,
            invited_by_user_id: team_role.user_id,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "teams.invitations.create",
        source,
    })?;

    Ok(ok_response(json!({
        "invitation": {
            "id": invitation.id,
            "team_id": invitation.team_id,
            "email": invitation.email,
            "role": super::role_value(&invitation.role),
            "status": "pending",
            "expires_at": invitation.expires_at,
        }
    })))
}
