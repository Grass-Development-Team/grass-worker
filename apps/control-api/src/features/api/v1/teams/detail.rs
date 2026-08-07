use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::{
    domain::teams,
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    let db = super::database(&state, "teams.detail.no_database")?;
    let team = teams::get_by_id(db, team_role.team_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.detail",
            source,
        })?
        .ok_or_else(|| AppError::NotFound {
            op: "teams.detail.not_found",
            message: "team not found".to_owned(),
        })?;

    Ok(ok_response(json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
            "avatar_url": super::super::avatars::team_avatar_url(team.id, team.avatar_version),
            "kind": super::kind_value(&team.kind),
            "owner_user_id": team.owner_user_id,
            "group_id": team.group_id,
            "role": super::role_value(&team_role.role),
        }
    })))
}
