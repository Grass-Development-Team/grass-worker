use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::{notifications, projects},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, project},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/slug",
        axum::routing::patch(update_slug),
    )
}

#[derive(Deserialize)]
struct UpdateSlugRequest {
    slug: String,
    #[serde(default)]
    reason: Option<String>,
}

/// PATCH /api/v1/admin/projects/{project_id}/slug
async fn update_slug(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<UpdateSlugRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.slug.update";
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = crate::infra::audit::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let current = load_project(&transaction, project_id, OP).await?;
    let slug =
        grass_validator::normalize_slug(&body.slug).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    if slug != current.slug
        && project::Entity::find()
            .filter(project::Column::TeamId.eq(current.team_id))
            .filter(project::Column::Slug.eq(&slug))
            .filter(project::Column::DeletedAt.is_null())
            .filter(project::Column::Id.ne(current.id))
            .one(&transaction)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .is_some()
    {
        return Err(AppError::Conflict {
            op: OP,
            message: "project slug is already used by another project in this team".to_owned(),
        });
    }
    let reason = body.reason.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    });
    let mut active: project::ActiveModel = current.clone().into();
    active.slug = Set(slug.clone());
    let updated = active.update(&transaction).await.map_err(|source| {
        let source: anyhow::Error = source.into();
        if crate::infra::database::is_unique_violation(&source) {
            AppError::Conflict {
                op: OP,
                message: "project slug is already used by another project in this team".to_owned(),
            }
        } else {
            AppError::Infrastructure { op: OP, source }
        }
    })?;
    audits::create_platform_audit_event_with_changes(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(data.user_id),
            actor_node_id: None,
            team_id: Some(updated.team_id),
            action: "project.slug_updated".to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(updated.id),
            result: AuditEventResult::Success,
            reason: reason.clone(),
            metadata: json!({ "platform_admin": true }),
        },
        json!({ "before": { "slug": current.slug }, "after": { "slug": updated.slug } }),
    )
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    notifications::create_project_notification(
        &transaction,
        notifications::CreateProjectNotification {
            project: &updated,
            actor_user_id: data.user_id,
            action: "project.slug_updated",
            reason: reason.clone(),
            target_url: format!("/projects/{}", updated.id),
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
    Ok(ok_response(UpdateSlugResponse {
        project: UpdateSlugProjectResponse {
            id: updated.id,
            uuid: updated.id,
            slug: updated.slug.clone(),
        },
        reason,
    }))
}

async fn load_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
    op: &'static str,
) -> Result<project::Model, AppError> {
    projects::get_by_id(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "project not found".to_owned(),
        })
}

#[derive(serde::Serialize)]
struct UpdateSlugProjectResponse {
    id: uuid::Uuid,
    uuid: uuid::Uuid,
    slug: String,
}

#[derive(serde::Serialize)]
struct UpdateSlugResponse {
    project: UpdateSlugProjectResponse,
    reason: Option<String>,
}
