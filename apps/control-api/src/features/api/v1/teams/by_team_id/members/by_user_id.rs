use axum::{Json, extract::Path, response::IntoResponse};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{
        quotas::QuotaDimension,
        teams::{self, MemberMutationError},
    },
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
        quota::{QuotaCharge, QuotaService},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/teams/{team_id}/members/{user_id}",
        axum::routing::delete(remove).patch(update_role),
    )
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

#[derive(Deserialize)]
pub struct MemberPath {
    pub user_id: Uuid,
}

#[derive(Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

pub async fn update_role(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Path(path): Path<MemberPath>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.members.update_role.admin_required")?;

    let role = parse_role(&body.role, "teams.members.update_role.invalid_role")?;
    let db = crate::infra::http::database(&state, "teams.members.update_role.no_database")?;
    let member = teams::update_member_role(db, team_role.team_id, path.user_id, role)
        .await
        .map_err(map_member_mutation_error)?;

    Ok(ok_response(UpdateRoleResponse {
        member: UpdateRoleMemberResponse {
            id: member.id,
            user_id: member.user_id,
            role: role_value(&member.role),
        },
    }))
}

pub async fn remove(
    axum::extract::State(state): axum::extract::State<ControlApiState>,
    team_role: TeamRole,
    Path(path): Path<MemberPath>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_admin("teams.members.remove.admin_required")?;

    let db = crate::infra::http::database(&state, "teams.members.remove.no_database")?;
    teams::remove_member(db, team_role.team_id, path.user_id)
        .await
        .map_err(map_member_mutation_error)?;

    if let Ok(cache) = crate::infra::http::cache(&state, "teams.members.remove.no_cache") {
        QuotaService::new(db, cache)
            .release(
                "teams.members.remove.quota_release",
                team_role.team_id,
                &[QuotaCharge::one(QuotaDimension::Members)],
                "team_member",
                Some(path.user_id),
            )
            .await?;
    }

    Ok(ok_response(RemoveResponse { ok: true }))
}

fn map_member_mutation_error(error: MemberMutationError) -> AppError {
    match error {
        MemberMutationError::NotFound => AppError::NotFound {
            op: "teams.members.not_found",
            message: error.to_string(),
        },
        MemberMutationError::OwnerConflict(message) => AppError::Conflict {
            op: "teams.members.owner_conflict",
            message,
        },
        MemberMutationError::Database(source) => AppError::Infrastructure {
            op: "teams.members.database",
            source: source.into(),
        },
    }
}

#[derive(serde::Serialize)]
struct UpdateRoleMemberResponse {
    id: uuid::Uuid,
    user_id: uuid::Uuid,
    role: &'static str,
}

#[derive(serde::Serialize)]
struct UpdateRoleResponse {
    member: UpdateRoleMemberResponse,
}

#[derive(serde::Serialize)]
struct RemoveResponse {
    ok: bool,
}
