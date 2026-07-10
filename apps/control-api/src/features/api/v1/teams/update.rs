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
    }

    if let Some(slug) = &body.slug {
        super::validate_required(slug, "teams.update.invalid_slug", "slug")?;
    }

    let db = super::database(&state, "teams.update.no_database")?;
    let team = teams::update(
        db,
        team_role.team_id,
        UpdateTeamParams {
            slug: body.slug.map(|slug| slug.trim().to_owned()),
            name: body.name.map(|name| name.trim().to_owned()),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "teams.update",
        source,
    })?;

    Ok(ok_response(json!({
        "team": {
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
            "kind": super::kind_value(&team.kind),
            "owner_user_id": team.owner_user_id,
            "group_id": team.group_id,
        }
    })))
}
