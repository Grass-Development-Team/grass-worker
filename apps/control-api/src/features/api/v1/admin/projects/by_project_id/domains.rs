use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::{
    domain::{hosts, projects},
    infra::{
        database::entity::{
            HostBindingKind, HostBindingStatus, HostReviewStatus, project, project_host_binding,
        },
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/domains",
        axum::routing::get(domains),
    )
}

fn admin_binding_view(binding: &project_host_binding::Model) -> HostBindingResponse {
    HostBindingResponse {
        id: binding.id,
        project_id: binding.project_id,
        host: binding.host.clone(),
        region: binding.region.clone(),
        kind: match binding.kind {
            HostBindingKind::Platform => "platform",
            HostBindingKind::Custom => "custom",
        },
        environment: match binding.environment {
            crate::infra::database::entity::HostBindingEnvironment::Production => "production",
            crate::infra::database::entity::HostBindingEnvironment::Preview => "preview",
            crate::infra::database::entity::HostBindingEnvironment::All => "all",
        },
        status: match binding.status {
            HostBindingStatus::Pending => "pending",
            HostBindingStatus::Active => "active",
            HostBindingStatus::Failed => "failed",
            HostBindingStatus::Disabled => "disabled",
        },
        review_status: match binding.review_status {
            HostReviewStatus::NotRequired => "not_required",
            HostReviewStatus::Pending => "pending",
            HostReviewStatus::Approved => "approved",
            HostReviewStatus::Rejected => "rejected",
        },
        failure_reason: binding.failure_reason.clone(),
        is_primary: binding.is_primary,
        host_source_id: binding.host_source_id,
        reviewed_by_user_id: binding.reviewed_by_user_id,
        reviewed_at: binding.reviewed_at,
        review_reason: binding.review_reason.clone(),
        created_at: binding.created_at,
        updated_at: binding.updated_at,
    }
}

/// GET /api/v1/admin/projects/{project_id}/domains
async fn domains(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.domains";
    let db = crate::infra::http::database(&state, OP)?;
    let _project = load_project_any(db, project_id, OP).await?;
    let bindings = hosts::list_bindings_for_project(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(DomainsResponse {
        domains: bindings.iter().map(admin_binding_view).collect::<Vec<_>>(),
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
struct HostBindingResponse {
    id: uuid::Uuid,
    project_id: uuid::Uuid,
    host: String,
    region: String,
    kind: &'static str,
    environment: &'static str,
    status: &'static str,
    review_status: &'static str,
    failure_reason: Option<String>,
    is_primary: bool,
    host_source_id: Option<uuid::Uuid>,
    reviewed_by_user_id: Option<uuid::Uuid>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    reviewed_at: Option<time::OffsetDateTime>,
    review_reason: Option<String>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DomainsResponse {
    domains: Vec<HostBindingResponse>,
}
