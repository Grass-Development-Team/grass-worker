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
    axum::Router::new().route(
        "/teams/{team_id}/quota-plan",
        axum::routing::post(set_quota_plan),
    )
}

#[derive(Deserialize)]
struct SetQuotaPlanRequest {
    /// `null` clears the override so the team inherits its group plan.
    plan_id: Option<Uuid>,
}

/// POST /api/v1/admin/teams/{team_id}/quota-plan
///
/// Sets or clears the explicit per-team quota plan override, which wins
/// over the team group's plan during resolution.
async fn set_quota_plan(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(team_id): Path<Uuid>,
    Json(body): Json<SetQuotaPlanRequest>,
) -> Result<impl IntoResponse, AppError> {
    let team =
        crate::domain::admin_teams::set_quota_plan(&state, data.user_id, team_id, body.plan_id)
            .await?;
    Ok(ok_response(SetQuotaPlanResponse {
        team: SetQuotaPlanTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            explicit_quota_plan_id: team.explicit_quota_plan_id,
        },
    }))
}

#[derive(serde::Serialize)]
struct SetQuotaPlanTeamResponse {
    id: uuid::Uuid,
    slug: String,
    explicit_quota_plan_id: Option<uuid::Uuid>,
}

#[derive(serde::Serialize)]
struct SetQuotaPlanResponse {
    team: SetQuotaPlanTeamResponse,
}
