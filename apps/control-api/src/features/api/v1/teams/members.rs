use axum::{Json, extract::Path, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::teams,
    infra::{
        database::entity::TeamMemberRole,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct MemberPath {
    pub user_id: Uuid,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

pub async fn list(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    let db = super::database(&state, "teams.members.list.no_database")?;
    let members = teams::list_members(db, team_role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.list",
            source,
        })?;

    Ok(ok_response(json!({
        "members": members.into_iter().map(|(member, user)| json!({
            "id": member.id,
            "user_id": user.id,
            "email": user.email,
            "display_name": user.display_name,
            "role": super::role_value(&member.role),
            "joined_at": member.joined_at,
        })).collect::<Vec<_>>()
    })))
}

pub async fn update_role(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Path(path): Path<MemberPath>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.members.update_role.admin_required")?;

    let role = super::parse_role(&body.role, "teams.members.update_role.invalid_role")?;
    let db = super::database(&state, "teams.members.update_role.no_database")?;
    let current_role = teams::member_role(db, team_role.team_id, path.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.update_role.current_role",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.members.update_role.not_found",
            message: "team member not found".to_owned(),
        })?;

    if matches!(role, TeamMemberRole::Owner) || matches!(current_role, TeamMemberRole::Owner) {
        team_role.require_owner("teams.members.update_role.owner_required")?;
    }

    if matches!(current_role, TeamMemberRole::Owner) && !matches!(role, TeamMemberRole::Owner) {
        ensure_not_last_owner(db, team_role.team_id).await?;
    }

    let member = teams::update_member_role(db, team_role.team_id, path.user_id, role)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.update_role",
            source,
        })?;

    Ok(ok_response(json!({
        "member": {
            "id": member.id,
            "user_id": member.user_id,
            "role": super::role_value(&member.role),
        }
    })))
}

pub async fn remove(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Path(path): Path<MemberPath>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.members.remove.admin_required")?;

    let db = super::database(&state, "teams.members.remove.no_database")?;
    let current_role = teams::member_role(db, team_role.team_id, path.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.remove.current_role",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.members.remove.not_found",
            message: "team member not found".to_owned(),
        })?;

    if matches!(current_role, TeamMemberRole::Owner) {
        team_role.require_owner("teams.members.remove.owner_required")?;
        ensure_not_last_owner(db, team_role.team_id).await?;
    }

    teams::remove_member(db, team_role.team_id, path.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.remove",
            source,
        })?;

    Ok(ok_response(json!({ "ok": true })))
}

async fn ensure_not_last_owner(
    db: &sea_orm::DatabaseConnection,
    team_id: Uuid,
) -> Result<(), AppError> {
    let owner_count = teams::active_owner_count(db, team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.members.owner_count",
            source,
        })?;

    if owner_count <= 1 {
        return Err(AppError::Conflict {
            op: "teams.members.last_owner",
            message: "team must keep at least one owner".to_owned(),
        });
    }

    Ok(())
}
