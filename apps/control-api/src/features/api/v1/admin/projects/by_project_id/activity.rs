use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::projects,
    infra::{
        database::entity::{
            AuditActorType, AuditEventResult, AuditEventVisibility, audit_event, deployment,
            project, project_host_binding,
        },
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/activity",
        axum::routing::get(activity),
    )
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

#[derive(Deserialize, Default)]
struct ActivityQuery {
    page: Option<u64>,
    per_page: Option<u64>,
}

fn binding_audit_target_types() -> [&'static str; 2] {
    ["host", "project_host_binding"]
}

/// GET /api/v1/admin/projects/{project_id}/activity
async fn activity(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ActivityQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.activity";
    let db = crate::infra::http::database(&state, OP)?;
    let _project = load_project_any(db, project_id, OP).await?;
    let deployment_ids = deployment::Entity::find()
        .select_only()
        .column(deployment::Column::Id)
        .filter(deployment::Column::ProjectId.eq(project_id))
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let binding_ids = project_host_binding::Entity::find()
        .select_only()
        .column(project_host_binding::Column::Id)
        .filter(project_host_binding::Column::ProjectId.eq(project_id))
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let mut targets = Condition::any().add(
        Condition::all()
            .add(audit_event::Column::TargetType.eq("project"))
            .add(audit_event::Column::TargetId.eq(project_id)),
    );
    if !deployment_ids.is_empty() {
        targets = targets.add(
            Condition::all()
                .add(audit_event::Column::TargetType.eq("deployment"))
                .add(audit_event::Column::TargetId.is_in(deployment_ids)),
        );
    }
    if !binding_ids.is_empty() {
        targets = targets.add(
            Condition::all()
                .add(audit_event::Column::TargetType.is_in(binding_audit_target_types()))
                .add(audit_event::Column::TargetId.is_in(binding_ids)),
        );
    }
    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100);
    let base = audit_event::Entity::find().filter(targets);
    let total = base
        .clone()
        .count(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let events = base
        .order_by_desc(audit_event::Column::CreatedAt)
        .order_by_desc(audit_event::Column::Id)
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(ActivityResponse {
        events: events.iter().map(event_view).collect::<Vec<_>>(),
        pagination: ActivityPaginationResponse {
            page,
            per_page,
            total,
            total_pages: total.div_ceil(per_page),
        },
    }))
}

async fn load_project_any<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    op: &'static str,
) -> Result<project::Model, AppError> {
    projects::get_by_id_any(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })
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
struct ActivityPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct ActivityResponse {
    events: Vec<AuditEventResponse>,
    pagination: ActivityPaginationResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_activity_includes_both_host_audit_target_names() {
        assert_eq!(
            binding_audit_target_types(),
            ["host", "project_host_binding"]
        );
    }
}
