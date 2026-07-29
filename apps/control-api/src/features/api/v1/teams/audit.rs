use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    domain::audits::{self, AuditEventFilter},
    infra::{
        database::entity::AuditEventVisibility,
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct TeamAuditQuery {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
}

/// GET /api/v1/teams/{team_id}/audit-events
pub async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Query(query): Query<TeamAuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.audit_events.list";
    let db = super::database(&state, OP)?;

    let events = audits::list_events(
        db,
        AuditEventFilter {
            action: query.action.filter(|action| !action.trim().is_empty()),
            target_id: None,
            team_id: Some(role.team_id),
            visibility: Some(AuditEventVisibility::Team),
            limit: query.limit.unwrap_or(100),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "events": events
            .iter()
            .map(crate::features::api::v1::admin::audit_events::event_view)
            .collect::<Vec<_>>(),
    })))
}
