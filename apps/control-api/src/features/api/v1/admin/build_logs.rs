use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{audits, cleanup},
    infra::{
        database::entity::AuditEventResult,
        error::{AppError, ok_response},
        http::extractors::Session,
        storage::LocalStorage,
    },
    state::ControlApiState,
};

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
    let created_from =
        super::audit_events::timestamp_from_millis(query.created_from_ms, "from", op)?;
    let created_to = super::audit_events::timestamp_from_millis(query.created_to_ms, "to", op)?;
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
    let db = super::database(&state, OP)?;
    let summary = cleanup::summarize_build_logs(db, &filter(query, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    Ok(ok_response(json!({
        "matched": summary.matched,
        "deletable": summary.deletable,
        "skipped": summary.skipped,
    })))
}

/// DELETE /api/v1/admin/cleanup/build-logs
pub async fn cleanup(
    State(state): State<ControlApiState>,
    session: Session,
    Json(query): Json<BuildLogQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.cleanup.build_logs.delete";
    let db = super::database(&state, OP)?;
    let storage_root = state.config.read().unwrap().storage.root.clone();
    let result =
        cleanup::delete_build_logs(db, &LocalStorage::new(storage_root), &filter(query, OP)?)
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

    Ok(ok_response(json!({
        "deleted": result.deleted,
        "failed": result.failed,
        "skipped": result.skipped,
    })))
}
