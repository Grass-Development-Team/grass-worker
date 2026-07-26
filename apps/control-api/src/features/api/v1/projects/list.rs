use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{projects, teams},
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct ListProjectsQuery {
    pub team_id: Uuid,
}

/// GET /api/v1/projects?team_id=...
pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
    Query(query): Query<ListProjectsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.list";
    let db = super::database(&state, OP)?;

    teams::member_role(db, query.team_id, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::Forbidden {
            op: OP,
            message: "not a member of this team".to_owned(),
        })?;

    let projects = projects::list_for_team(db, query.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "projects": projects.iter().map(super::project_view).collect::<Vec<_>>(),
    })))
}
