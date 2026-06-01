use axum::{Json, extract::State, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::teams::{self, CreateTeamParams},
    infra::{
        database::entity::TeamKind,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct CreateTeamRequest {
    pub name: String,
    pub slug: String,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
    Json(body): Json<CreateTeamRequest>,
) -> Result<impl IntoResponse, AppError> {
    super::validate_required(&body.name, "teams.create.invalid_name", "name")?;
    super::validate_required(&body.slug, "teams.create.invalid_slug", "slug")?;

    let db = super::database(&state, "teams.create.no_database")?;
    let team = teams::create_team(
        db,
        CreateTeamParams {
            slug: body.slug.trim().to_owned(),
            name: body.name.trim().to_owned(),
            kind: TeamKind::Team,
            owner_user_id: session.data.user_id,
            group_id: None,
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure {
        op: "teams.create",
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
