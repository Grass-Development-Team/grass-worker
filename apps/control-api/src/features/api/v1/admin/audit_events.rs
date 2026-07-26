use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::audits::{self, AuditEventFilter},
    infra::{
        database::entity::{AuditEventResult, audit_event},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub target_id: Option<Uuid>,
    #[serde(default)]
    pub team_id: Option<Uuid>,
    #[serde(default)]
    pub limit: Option<u64>,
}

pub(crate) fn event_view(event: &audit_event::Model) -> serde_json::Value {
    json!({
        "id": event.id,
        "actor_user_id": event.actor_user_id,
        "team_id": event.team_id,
        "action": event.action,
        "target_type": event.target_type,
        "target_id": event.target_id,
        "result": match event.result {
            AuditEventResult::Success => "success",
            AuditEventResult::Failure => "failure",
            AuditEventResult::Denied => "denied",
        },
        "reason": event.reason,
        "metadata": event.metadata,
        "created_at": ts(event.created_at),
    })
}

/// GET /api/v1/admin/audit-events
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.audit_events.list";
    let db = super::database(&state, OP)?;

    let events = audits::list_events(
        db,
        AuditEventFilter {
            action: query.action.filter(|action| !action.trim().is_empty()),
            target_id: query.target_id,
            team_id: query.team_id,
            limit: query.limit.unwrap_or(100),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "events": events.iter().map(event_view).collect::<Vec<_>>(),
    })))
}
