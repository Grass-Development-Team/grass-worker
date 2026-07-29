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
        database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility, audit_event},
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
        "actor_node_id": event.actor_node_id,
        "team_id": event.team_id,
        "actor_type": match event.actor_type {
            AuditActorType::Anonymous => "anonymous",
            AuditActorType::User => "user",
            AuditActorType::System => "system",
            AuditActorType::Node => "node",
        },
        "visibility": match event.visibility {
            AuditEventVisibility::Platform => "platform",
            AuditEventVisibility::Team => "team",
        },
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
        "request_id": event.request_id,
        "source_ip": event.source_ip,
        "user_agent": event.user_agent,
        "http_method": event.http_method,
        "request_path": event.request_path,
        "status_code": event.status_code,
        "duration_ms": event.duration_ms,
        "changes": event.changes,
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
            visibility: None,
            limit: query.limit.unwrap_or(100),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "events": events.iter().map(event_view).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use crate::infra::database::entity::{AuditActorType, AuditEventVisibility, audit_event};

    use super::*;

    #[test]
    fn event_view_exposes_complete_request_context() {
        let request_id = Uuid::now_v7();
        let event = audit_event::Model {
            id: Uuid::now_v7(),
            actor_user_id: Some(Uuid::now_v7()),
            actor_node_id: None,
            team_id: Some(Uuid::now_v7()),
            actor_type: AuditActorType::User,
            visibility: AuditEventVisibility::Platform,
            action: "projects.detail.update".to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(Uuid::now_v7()),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "matched_path": "/api/v1/projects/{project_id}" }),
            request_id: Some(request_id),
            source_ip: Some("192.0.2.10".to_owned()),
            user_agent: Some("Grass Console".to_owned()),
            http_method: Some("PATCH".to_owned()),
            request_path: Some("/api/v1/projects/0196".to_owned()),
            status_code: Some(200),
            duration_ms: Some(17),
            changes: json!({ "before": { "name": "Old" }, "after": { "name": "New" } }),
            created_at: OffsetDateTime::UNIX_EPOCH,
        };

        let view = event_view(&event);

        assert_eq!(view["actor_type"], "user");
        assert_eq!(view["visibility"], "platform");
        assert_eq!(view["request_id"], request_id.to_string());
        assert_eq!(view["source_ip"], "192.0.2.10");
        assert_eq!(view["http_method"], "PATCH");
        assert_eq!(view["status_code"], 200);
        assert_eq!(view["duration_ms"], 17);
        assert_eq!(view["changes"]["after"]["name"], "New");
    }
}
