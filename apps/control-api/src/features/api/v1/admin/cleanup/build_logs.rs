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
    domain::cleanup,
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/cleanup/build-logs",
        axum::routing::delete(cleanup).get(cleanup_preview),
    )
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

#[derive(Deserialize, Default)]
pub struct BuildLogQuery {
    #[serde(default)]
    pub deployment_id: Option<Uuid>,
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[serde(default)]
    pub team_id: Option<Uuid>,
    #[serde(default)]
    pub triggered_by_user_id: Option<Uuid>,
    #[serde(default, rename = "from")]
    pub created_from_ms: Option<i64>,
    #[serde(default, rename = "to")]
    pub created_to_ms: Option<i64>,
}

fn filter(query: BuildLogQuery, op: &'static str) -> Result<cleanup::BuildLogFilter, AppError> {
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

    Ok(cleanup::BuildLogFilter {
        deployment_id: query.deployment_id,
        project_id: query.project_id,
        team_id: query.team_id,
        triggered_by_user_id: query.triggered_by_user_id,
        created_from,
        created_to,
    })
}

/// GET /api/v1/admin/cleanup/build-logs
pub async fn cleanup_preview(
    State(state): State<ControlApiState>,
    Query(query): Query<BuildLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.build_logs.preview";
    let db = crate::infra::http::database(&state, OP)?;
    let summary = cleanup::summarize_build_logs(db, &filter(query, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(CleanupPreviewResponse {
        matched: summary.matched,
        deletable: summary.deletable,
        skipped: summary.skipped,
    }))
}

/// DELETE /api/v1/admin/cleanup/build-logs
pub async fn cleanup(
    State(state): State<ControlApiState>,
    session: Session,
    Json(query): Json<BuildLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.build_logs.delete";
    let db = crate::infra::http::database(&state, OP)?;
    let result = cleanup::delete_build_logs(db, &state.storage, &filter(query, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    audits::create_platform_audit_event(
        db,
        audits::CreateAuditEventParams {
            actor_user_id: Some(session.data.user_id),
            actor_node_id: None,
            team_id: None,
            action: "admin.cleanup.build_logs".to_owned(),
            target_type: "deployment_artifact".to_owned(),
            target_id: None,
            result: if result.failed == 0 {
                AuditEventResult::Success
            } else {
                AuditEventResult::Failure
            },
            reason: None,
            metadata: json!({
                "deleted": result.deleted,
                "failed": result.failed,
                "skipped": result.skipped,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(CleanupResponse {
        deleted: result.deleted,
        failed: result.failed,
        skipped: result.skipped,
    }))
}

#[derive(serde::Serialize)]
struct CleanupPreviewResponse {
    matched: u64,
    deletable: u64,
    skipped: u64,
}

#[derive(serde::Serialize)]
struct CleanupResponse {
    deleted: u64,
    failed: u64,
    skipped: u64,
}
