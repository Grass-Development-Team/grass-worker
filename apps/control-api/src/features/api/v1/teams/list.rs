use axum::{extract::State, response::IntoResponse};
use serde_json::json;

use crate::{
    domain::teams,
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub async fn handler(
    State(state): State<ControlApiState>,
    session: Session,
) -> Result<impl IntoResponse, AppError> {
    let db = super::database(&state, "teams.list.no_database")?;
    let teams = teams::list_for_user(db, session.data.user_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "teams.list",
            source,
        })?;

    Ok(ok_response(json!({
        "teams": teams.into_iter().map(|team| json!({
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
            "kind": super::kind_value(&team.kind),
            "owner_user_id": team.owner_user_id,
            "group_id": team.group_id,
        })).collect::<Vec<_>>()
    })))
}
