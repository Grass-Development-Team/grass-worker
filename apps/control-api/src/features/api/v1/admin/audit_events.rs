use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::AuditEventFilter,
        database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility, audit_event},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/audit-events", axum::routing::get(list))
}

#[derive(Default, Deserialize)]
struct AuditQuery {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    actor_user_id: Option<Uuid>,
    #[serde(default)]
    actor_type: Option<String>,
    #[serde(default)]
    target_type: Option<String>,
    #[serde(default)]
    target_id: Option<Uuid>,
    #[serde(default)]
    team_id: Option<Uuid>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default, rename = "from")]
    created_from_ms: Option<i64>,
    #[serde(default, rename = "to")]
    created_to_ms: Option<i64>,
    #[serde(default)]
    page: Option<u64>,
    #[serde(default)]
    per_page: Option<u64>,
    #[serde(default)]
    #[serde(rename = "snapshot_before")]
    _snapshot_before: Option<i64>,
}

fn parse_actor_type(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<AuditActorType>, AppError> {
    value
        .map(|value| match value {
            "anonymous" => Ok(AuditActorType::Anonymous),
            "user" => Ok(AuditActorType::User),
            "system" => Ok(AuditActorType::System),
            "node" => Ok(AuditActorType::Node),
            _ => Err(AppError::Validation {
                op,
                message: "actor_type must be anonymous, user, system, or node".to_owned(),
            }),
        })
        .transpose()
}

fn parse_result(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<AuditEventResult>, AppError> {
    value
        .map(|value| match value {
            "success" => Ok(AuditEventResult::Success),
            "failure" => Ok(AuditEventResult::Failure),
            "denied" => Ok(AuditEventResult::Denied),
            _ => Err(AppError::Validation {
                op,
                message: "result must be success, failure, or denied".to_owned(),
            }),
        })
        .transpose()
}

fn timestamp_from_millis(
    value: Option<i64>,
    field: &'static str,
    op: &'static str,
) -> Result<Option<OffsetDateTime>, AppError> {
    value
        .map(|value| {
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(value) * 1_000_000).map_err(|_| {
                AppError::Validation {
                    op,
                    message: format!("{field} is outside the supported timestamp range"),
                }
            })
        })
        .transpose()
}

fn event_filter(
    query: AuditQuery,
    team_id: Option<Uuid>,
    visibility: Option<AuditEventVisibility>,
    team_visible_only: bool,
    op: &'static str,
) -> Result<AuditEventFilter, AppError> {
    let created_from = timestamp_from_millis(query.created_from_ms, "from", op)?;
    let created_to = timestamp_from_millis(query.created_to_ms, "to", op)?;
    if created_from
        .zip(created_to)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(AppError::Validation {
            op,
            message: "from must not be later than to".to_owned(),
        });
    }
    Ok(AuditEventFilter {
        action: query.action.filter(|value| !value.trim().is_empty()),
        actor_user_id: query.actor_user_id,
        actor_type: parse_actor_type(query.actor_type.as_deref(), op)?,
        target_type: query.target_type.filter(|value| !value.trim().is_empty()),
        target_id: query.target_id,
        team_id: team_id.or(query.team_id),
        result: parse_result(query.result.as_deref(), op)?,
        created_from,
        created_to,
        visibility,
        team_visible_only,
        page: query.page.unwrap_or(1),
        per_page: query.per_page.unwrap_or(50),
    })
}

fn event_view(event: &audit_event::Model) -> AuditEventResponse {
    AuditEventResponse {
        id: event.id,
        actor_user_id: event.actor_user_id,
        actor_node_id: event.actor_node_id,
        team_id: event.team_id,
        actor_type: match event.actor_type {
            AuditActorType::Anonymous => "anonymous",
            AuditActorType::User => "user",
            AuditActorType::System => "system",
            AuditActorType::Node => "node",
        },
        visibility: match event.visibility {
            AuditEventVisibility::Platform => "platform",
            AuditEventVisibility::Team => "team",
        },
        action: event.action.clone(),
        target_type: event.target_type.clone(),
        target_id: event.target_id,
        result: match event.result {
            AuditEventResult::Success => "success",
            AuditEventResult::Failure => "failure",
            AuditEventResult::Denied => "denied",
        },
        reason: event.reason.clone(),
        metadata: event.metadata.clone(),
        request_id: event.request_id,
        source_ip: event.source_ip.clone(),
        user_agent: event.user_agent.clone(),
        http_method: event.http_method.clone(),
        request_path: event.request_path.clone(),
        status_code: event.status_code,
        duration_ms: event.duration_ms,
        changes: event.changes.clone(),
        created_at: event.created_at,
    }
}

/// GET /api/v1/admin/audit-events
async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.audit_events.list";
    let db = crate::infra::http::database(&state, OP)?;

    let page = audits::list_events(db, event_filter(query, None, None, false, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(ListResponse {
        events: page.events.iter().map(event_view).collect::<Vec<_>>(),
        pagination: ListPaginationResponse {
            page: page.page,
            per_page: page.per_page,
            total: page.total,
            total_pages: page.total_pages,
        },
    }))
}

#[derive(serde::Serialize)]
struct AuditEventResponse {
    id: uuid::Uuid,
    actor_user_id: Option<uuid::Uuid>,
    actor_node_id: Option<uuid::Uuid>,
    team_id: Option<uuid::Uuid>,
    actor_type: &'static str,
    visibility: &'static str,
    action: String,
    target_type: String,
    target_id: Option<uuid::Uuid>,
    result: &'static str,
    reason: Option<String>,
    metadata: serde_json::Value,
    request_id: Option<uuid::Uuid>,
    source_ip: Option<String>,
    user_agent: Option<String>,
    http_method: Option<String>,
    request_path: Option<String>,
    status_code: Option<i32>,
    duration_ms: Option<i64>,
    changes: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct ListResponse {
    events: Vec<AuditEventResponse>,
    pagination: ListPaginationResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::AuditActorType;
    use crate::infra::database::entity::AuditEventResult;
    use crate::infra::database::entity::AuditEventVisibility;
    use crate::infra::database::entity::audit_event;
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;
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

        let view = serde_json::to_value(event_view(&event)).unwrap();

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
