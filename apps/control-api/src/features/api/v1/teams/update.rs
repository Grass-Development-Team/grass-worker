use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::teams::{self, UpdateTeamParams},
    infra::{
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct UpdateTeamRequest {
    pub name: Option<String>,
    pub slug: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    team_role: TeamRole,
    Json(body): Json<UpdateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    team_role.require_owner("teams.update.owner_required")?;

    if let Some(name) = &body.name {
        super::validate_required(name, "teams.update.invalid_name", "name")?;
        if name.trim().chars().count() > 160 {
            return Err(AppError::Validation {
                op: "teams.update.invalid_name",
                message: "team name must not exceed 160 characters".to_owned(),
            });
        }
    }

    let slug = body
        .slug
        .as_deref()
        .map(|slug| super::normalize_slug(slug, "teams.update.invalid_slug"))
        .transpose()?;

    let db = super::database(&state, "teams.update.no_database")?;
    let team = teams::update(
        db,
        team_role.team_id,
        UpdateTeamParams {
            slug,
            name: body.name.map(|name| name.trim().to_owned()),
        },
    )
    .await
    .map_err(|source| super::map_team_write_error(source, "teams.update"))?;

    Ok(ok_response(json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
            "avatar_url": super::super::avatars::team_avatar_url(team.id, team.avatar_version),
            "kind": super::kind_value(&team.kind),
            "owner_user_id": team.owner_user_id,
            "group_id": team.group_id,
        }
    })))
}
