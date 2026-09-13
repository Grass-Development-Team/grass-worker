use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    infra::{
        audit::AuditEventFilter,
        database::entity::{AuditActorType, AuditEventResult, AuditEventVisibility, audit_event},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/cleanup/audit-events",
        axum::routing::delete(cleanup).get(cleanup_preview),
    )
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
    snapshot_before: Option<i64>,
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

/// GET /api/v1/admin/cleanup/audit-events
async fn cleanup_preview(
    State(state): State<ControlApiState>,
    Query(query): Query<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.audit_events.preview";
    let db = crate::infra::http::database(&state, OP)?;
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

    Ok(ok_response(CleanupPreviewResponse {
        matched: page.total,
        deletable: page.total,
        skipped: 0,
        events: page.events.iter().map(event_view).collect::<Vec<_>>(),
        pagination: CleanupPreviewPaginationResponse {
            page: page.page,
            per_page: page.per_page,
            total: page.total,
            total_pages: page.total_pages,
        },
        snapshot_before: snapshot_ms,
    }))
}

/// DELETE /api/v1/admin/cleanup/audit-events
async fn cleanup(
    State(state): State<ControlApiState>,
    session: Session,
    Json(query): Json<AuditQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.audit_events.delete";
    let db = crate::infra::http::database(&state, OP)?;
    let (filter, _) = cleanup_event_filter(query, None, OP)?;
    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let deleted = audits::delete_events(&transaction, filter)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        &transaction,
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

    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(CleanupResponse {
        deleted,
        skipped: 0,
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
struct CleanupPreviewPaginationResponse {
    page: u64,
    per_page: u64,
    total: u64,
    total_pages: u64,
}

#[derive(serde::Serialize)]
struct CleanupPreviewResponse {
    matched: u64,
    deletable: u64,
    skipped: u64,
    events: Vec<AuditEventResponse>,
    pagination: CleanupPreviewPaginationResponse,
    snapshot_before: i64,
}

#[derive(serde::Serialize)]
struct CleanupResponse {
    deleted: u64,
    skipped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;
    async fn check_cleanup_transaction(fail_audit: bool) {
        use axum::{
            Extension,
            body::Body,
            http::{Request, StatusCode},
        };
        use sea_orm::{DbBackend, DbErr, MockDatabase, MockExecResult};
        use tower::ServiceExt;

        let mock = MockDatabase::new(DbBackend::Postgres).append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 3,
        }]);
        let mock = if fail_audit {
            mock.append_exec_errors([DbErr::Custom("audit insert unavailable".to_owned())])
        } else {
            mock.append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
        };
        let state = crate::state::ControlApiState::new(Default::default(), "unused.toml");
        state.database.set(mock.into_connection()).unwrap();
        let now = OffsetDateTime::now_utc();
        let session = Some((
            "test-session".to_owned(),
            grass_session::SessionData {
                user_id: Uuid::now_v7(),
                auth_version: 0,
                created_at: now,
                last_accessed_at: now,
            },
        ));
        let app = router().layer(Extension(session)).with_state(state.clone());
        let request = Request::builder()
            .method("DELETE")
            .uri("/cleanup/audit-events")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "snapshot_before": 0 }).to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            if fail_audit {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::OK
            }
        );
        let log = std::sync::Arc::try_unwrap(state.database)
            .unwrap()
            .into_inner()
            .unwrap()
            .into_transaction_log();
        let statements = format!("{log:?}");
        assert!(statements.contains("DELETE FROM"), "{statements}");
        assert!(statements.contains("INSERT INTO"), "{statements}");
        assert!(
            statements.contains(if fail_audit { "ROLLBACK" } else { "COMMIT" }),
            "{statements}"
        );
        assert!(
            !statements.contains(if fail_audit { "COMMIT" } else { "ROLLBACK" }),
            "{statements}"
        );
    }

    #[tokio::test]
    async fn cleanup_http_rolls_back_when_its_audit_cannot_be_written() {
        check_cleanup_transaction(true).await;
    }

    #[tokio::test]
    async fn cleanup_http_commits_deletion_and_its_audit_together() {
        check_cleanup_transaction(false).await;
    }

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
}
