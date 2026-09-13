use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/teams/{team_id}/group", axum::routing::post(assign))
}

#[derive(Deserialize)]
struct AssignGroupRequest {
    group_id: Uuid,
}

/// POST /api/v1/admin/teams/{team_id}/group
async fn assign(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
    Json(body): Json<AssignGroupRequest>,
) -> Result<impl IntoResponse, AppError> {
    let team =
        crate::domain::admin_team_groups::assign(&state, data.user_id, team_id, body.group_id)
            .await?;
    Ok(ok_response(AssignResponse {
        team: AssignTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            group_id: team.group_id,
        },
    }))
}

#[derive(serde::Serialize)]
struct AssignTeamResponse {
    id: uuid::Uuid,
    slug: String,
    group_id: Option<uuid::Uuid>,
}

#[derive(serde::Serialize)]
struct AssignResponse {
    team: AssignTeamResponse,
}
