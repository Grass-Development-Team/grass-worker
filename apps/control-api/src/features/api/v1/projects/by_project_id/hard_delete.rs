use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{project_lifecycle::record_lifecycle_audit, projects},
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/hard-delete",
        axum::routing::post(hard_delete),
    )
}

/// POST /api/v1/projects/{project_id}/hard-delete
pub async fn hard_delete(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hard_delete";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::IncludingDeleted,
        OP,
    )
    .await?;
    access.require_owner(OP)?;
    if access.project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project must be soft deleted before it can be hard deleted".to_owned(),
        });
    }
    let db = crate::infra::http::database(&state, OP)?;

    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    projects::hard_delete(&transaction, access.project.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;

    record_lifecycle_audit(
        &transaction,
        session.data.user_id,
        access.team.id,
        "project.hard_deleted",
        project_id,
        json!({}),
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
    Ok(ok_response(HardDeleteResponse { ok: true }))
}

#[derive(serde::Serialize)]
struct HardDeleteResponse {
    ok: bool,
}
