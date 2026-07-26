use axum::{Json, extract::Path, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
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
            "joined_at": ts(member.joined_at),
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
    let member = teams::update_member_role(db, team_role.team_id, path.user_id, role)
        .await
        .map_err(map_member_mutation_error)?;

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
    teams::remove_member(db, team_role.team_id, path.user_id)
        .await
        .map_err(map_member_mutation_error)?;

    if let Ok(cache) = super::cache(&state, "teams.members.remove.no_cache") {
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

    Ok(ok_response(json!({ "ok": true })))
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
