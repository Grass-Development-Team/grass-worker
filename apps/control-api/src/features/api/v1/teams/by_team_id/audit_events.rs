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
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/teams/{team_id}/audit-events", axum::routing::get(list))
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

fn authorize_team_audit(role: &TeamRole) -> Result<(), AppError> {
    role.require_admin("teams.audit_events.list.admin_required")
}

/// GET /api/v1/teams/{team_id}/audit-events
async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Query(query): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.audit_events.list";
    authorize_team_audit(&role)?;
    let db = crate::infra::http::database(&state, OP)?;

    let page = audits::list_events(
        db,
        event_filter(
            query,
            Some(role.team_id),
            Some(AuditEventVisibility::Team),
            true,
            OP,
        )?,
    )
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
    use crate::infra::database::entity::TeamMemberRole;
    use crate::infra::http::extractors::TeamRole;
    use uuid::Uuid;
    fn role(role: TeamMemberRole) -> TeamRole {
        TeamRole {
            team_id: Uuid::now_v7(),
            user_id: Uuid::now_v7(),
            role,
        }
    }

    #[test]
    fn team_audit_is_limited_to_owners_and_admins() {
        assert!(authorize_team_audit(&role(TeamMemberRole::Owner)).is_ok());
        assert!(authorize_team_audit(&role(TeamMemberRole::Admin)).is_ok());
        assert!(authorize_team_audit(&role(TeamMemberRole::Member)).is_err());
        assert!(authorize_team_audit(&role(TeamMemberRole::Viewer)).is_err());
    }
}
