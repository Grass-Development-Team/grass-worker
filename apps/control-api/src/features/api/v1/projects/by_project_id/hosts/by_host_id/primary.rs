use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::TransactionTrait;
use uuid::Uuid;

use crate::{
    domain::hosts,
    infra::{
        database::entity::project_host_binding,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/hosts/{host_id}/primary",
        axum::routing::post(set_primary),
    )
}

/// POST /api/v1/projects/{project_id}/hosts/{host_id}/primary
pub async fn set_primary(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, host_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.hosts.set_primary";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_member(OP)?;
    let db = crate::infra::http::database(&state, OP)?;

    let binding = load_binding(db, &access, host_id, OP).await?;

    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    hosts::set_primary_binding(&transaction, access.project.id, binding.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    Ok(ok_response(SetPrimaryResponse { ok: true }))
}

pub(super) async fn load_binding(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    host_id: Uuid,
    op: &'static str,
) -> Result<project_host_binding::Model, AppError> {
    let binding = hosts::get_binding_by_id(db, host_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        })?;
    if binding.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "host binding not found".to_owned(),
        });
    }
    Ok(binding)
}

#[derive(serde::Serialize)]
struct SetPrimaryResponse {
    ok: bool,
}
