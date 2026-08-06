use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::http::{extractors::Session, timestamps::ts};
use crate::{
    domain::audits::{self, AuditEventFilter},
    infra::{
        database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility, audit_event},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

#[derive(Default, Deserialize)]
pub struct AuditQuery {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub actor_user_id: Option<Uuid>,
    #[serde(default)]
    pub actor_type: Option<String>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub target_id: Option<Uuid>,
    #[serde(default)]
    pub team_id: Option<Uuid>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default, rename = "from")]
    pub created_from_ms: Option<i64>,
    #[serde(default, rename = "to")]
    pub created_to_ms: Option<i64>,
    #[serde(default)]
    pub page: Option<u64>,
    #[serde(default)]
    pub per_page: Option<u64>,
    #[serde(default)]
    pub snapshot_before: Option<i64>,
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

pub(crate) fn timestamp_from_millis(
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

pub(crate) fn event_filter(
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

fn cleanup_event_filter(
    query: AuditQuery,
    generated_snapshot_ms: Option<i64>,
    op: &'static str,
) -> Result<(AuditEventFilter, i64), AppError> {
    let snapshot_ms = query
        .snapshot_before
        .or(generated_snapshot_ms)
        .ok_or_else(|| AppError::Validation {
            op,
            message: "snapshot_before from a cleanup preview is required".to_owned(),
        })?;
    let snapshot = timestamp_from_millis(Some(snapshot_ms), "snapshot_before", op)?
        .expect("snapshot_before is present");
    if snapshot > OffsetDateTime::now_utc() {
        return Err(AppError::Validation {
            op,
            message: "snapshot_before must not be in the future".to_owned(),
        });
    }
    let mut filter = event_filter(query, None, None, false, op)?;
    filter.created_to = Some(
        filter
            .created_to
            .map_or(snapshot, |created_to| created_to.min(snapshot)),
    );
    if filter
        .created_from
        .is_some_and(|created_from| created_from > snapshot)
    {
        return Err(AppError::Validation {
            op,
            message: "from must not be later than snapshot_before".to_owned(),
        });
    }
    Ok((filter, snapshot_ms))
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

    let page = audits::list_events(db, event_filter(query, None, None, false, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "events": page.events.iter().map(event_view).collect::<Vec<_>>(),
        "pagination": {
            "page": page.page,
            "per_page": page.per_page,
            "total": page.total,
            "total_pages": page.total_pages,
        },
    })))
}

/// GET /api/v1/admin/cleanup/audit-events
pub async fn cleanup_preview(
    State(state): State<ControlApiState>,
    Query(query): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.audit_events.preview";
    let db = super::database(&state, OP)?;
    let now = OffsetDateTime::now_utc();
    let snapshot_ms = i64::try_from(now.unix_timestamp_nanos() / 1_000_000).map_err(|_| {
        AppError::Infrastructure {
            op: OP,
            source: anyhow::anyhow!("current timestamp is outside the supported millisecond range"),
        }
    })?;
    let (filter, snapshot_ms) = cleanup_event_filter(query, Some(snapshot_ms), OP)?;
    let page = audits::list_events(db, filter)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "matched": page.total,
        "deletable": page.total,
        "skipped": 0,
        "events": page.events.iter().map(event_view).collect::<Vec<_>>(),
        "pagination": {
            "page": page.page,
            "per_page": page.per_page,
            "total": page.total,
            "total_pages": page.total_pages,
        },
        "snapshot_before": snapshot_ms,
    })))
}

/// DELETE /api/v1/admin/cleanup/audit-events
pub async fn cleanup(
    State(state): State<ControlApiState>,
    session: Session,
    Json(query): Json<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.audit_events.delete";
    let db = super::database(&state, OP)?;
    let (filter, _) = cleanup_event_filter(query, None, OP)?;
    let deleted = audits::delete_events(db, filter)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        audits::CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "admin.cleanup.audit_events".to_owned(),
            target_type: "audit_event".to_owned(),
            target_id: None,
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "deleted": deleted }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "deleted": deleted,
        "skipped": 0,
    })))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use crate::infra::database::entity::{AuditActorType, AuditEventVisibility, audit_event};

    use super::*;

    #[test]
    fn cleanup_delete_requires_a_preview_snapshot() {
        let error = cleanup_event_filter(AuditQuery::default(), None, "test.cleanup")
            .err()
            .expect("missing snapshot must fail");

        assert!(error.to_string().contains("snapshot_before"));
    }

    #[test]
    fn cleanup_snapshot_caps_the_requested_time_range() {
        let snapshot_ms = 1_000;
        let (filter, returned_snapshot) = cleanup_event_filter(
            AuditQuery {
                created_to_ms: Some(2_000),
                snapshot_before: Some(snapshot_ms),
                ..Default::default()
            },
            None,
            "test.cleanup",
        )
        .unwrap();

        assert_eq!(returned_snapshot, snapshot_ms);
        assert_eq!(
            filter.created_to,
            timestamp_from_millis(Some(snapshot_ms), "snapshot_before", "test.cleanup").unwrap()
        );
    }

    #[test]
    fn cleanup_rejects_a_future_snapshot() {
        let future_ms = (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
            .try_into()
            .unwrap_or(i64::MAX)
            .saturating_add(60_000);
        let error = match cleanup_event_filter(
            AuditQuery {
                snapshot_before: Some(future_ms),
                ..Default::default()
            },
            None,
            "test.cleanup",
        ) {
            Ok(_) => panic!("future snapshots must fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("future"));
    }

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
