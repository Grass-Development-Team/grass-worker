use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments, projects,
    },
    infra::{
        database::entity::{AuditEventResult, deployment, project, team},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

fn deployment_summary(deployment: &deployment::Model) -> serde_json::Value {
    json!({
        "id": deployment.id,
        "environment": deployments::environment_value(&deployment.environment),
        "build_status": deployments::build_status_value(&deployment.build_status),
        "release_status": deployments::release_status_value(&deployment.release_status),
        "created_at": ts(deployment.created_at),
    })
}

fn project_view(
    project: &project::Model,
    team: Option<&team::Model>,
    latest: Option<&deployment::Model>,
) -> serde_json::Value {
    json!({
        "id": project.id,
        "slug": project.slug,
        "name": project.name,
        "runtime": projects::runtime_value(&project.runtime),
        "repository_url": project.repository_url,
        "team": team.map(|team| json!({
            "id": team.id,
            "slug": team.slug,
            "name": team.name,
        })),
        "latest_deployment": latest.map(deployment_summary),
        "archived_at": ts(project.archived_at),
        "created_at": ts(project.created_at),
    })
}

#[derive(Deserialize)]
pub struct ListProjectsQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
}

/// GET /api/v1/admin/projects — every non-deleted project on the platform
/// with its team and most recent deployment.
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListProjectsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.list";
    let db = super::database(&state, OP)?;

    let mut select = project::Entity::find().filter(project::Column::DeletedAt.is_null());
    if let Some(term) = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|term| !term.is_empty())
    {
        let pattern = format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        select = select.filter(
            sea_orm::Condition::any()
                .add(project::Column::Slug.like(pattern.clone()))
                .add(project::Column::Name.like(pattern)),
        );
    }
    let projects_list = select
        .order_by_desc(project::Column::CreatedAt)
        .limit(query.limit.unwrap_or(100).clamp(1, 500))
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;

    let project_ids: Vec<Uuid> = projects_list.iter().map(|project| project.id).collect();
    let team_ids: Vec<Uuid> = projects_list
        .iter()
        .map(|project| project.team_id)
        .collect();

    let teams: HashMap<Uuid, team::Model> = if team_ids.is_empty() {
        HashMap::new()
    } else {
        team::Entity::find()
            .filter(team::Column::Id.is_in(team_ids))
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|team| (team.id, team))
            .collect()
    };

    // Latest deployment per project in one query (Postgres DISTINCT ON).
    let latest: HashMap<Uuid, deployment::Model> = if project_ids.is_empty() {
        HashMap::new()
    } else {
        deployment::Entity::find()
            .distinct_on([deployment::Column::ProjectId])
            .filter(deployment::Column::ProjectId.is_in(project_ids))
            .filter(deployment::Column::DeletedAt.is_null())
            .order_by_asc(deployment::Column::ProjectId)
            .order_by_desc(deployment::Column::CreatedAt)
            .all(db)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: OP,
                source: source.into(),
            })?
            .into_iter()
            .map(|deployment| (deployment.project_id, deployment))
            .collect()
    };

    Ok(ok_response(json!({
        "projects": projects_list
            .iter()
            .map(|project| project_view(
                project,
                teams.get(&project.team_id),
                latest.get(&project.id),
            ))
            .collect::<Vec<_>>(),
    })))
}

async fn load_project(
    db: &sea_orm::DatabaseConnection,
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

async fn record_project_audit(
    db: &sea_orm::DatabaseConnection,
    actor: Uuid,
    project: &project::Model,
    action: &str,
) {
    let _ = audits::create_platform_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor),
            actor_node_id: None,
            team_id: Some(project.team_id),
            action: action.to_owned(),
            target_type: "project".to_owned(),
            target_id: Some(project.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "platform_admin": true, "slug": project.slug }),
        },
    )
    .await;
}

/// POST /api/v1/admin/projects/{project_id}/archive
pub async fn archive(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.archive";
    let db = super::database(&state, OP)?;
    let project = load_project(db, project_id, OP).await?;
    if project.archived_at.is_some() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is already archived".to_owned(),
        });
    }
    let project = projects::set_archived(db, project, true)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_audit(db, data.user_id, &project, "project.archived").await;
    Ok(ok_response(
        json!({ "project": project_view(&project, None, None) }),
    ))
}

/// POST /api/v1/admin/projects/{project_id}/unarchive
pub async fn unarchive(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.unarchive";
    let db = super::database(&state, OP)?;
    let project = load_project(db, project_id, OP).await?;
    if project.archived_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is not archived".to_owned(),
        });
    }
    let project = projects::set_archived(db, project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_audit(db, data.user_id, &project, "project.unarchived").await;
    Ok(ok_response(
        json!({ "project": project_view(&project, None, None) }),
    ))
}

/// POST /api/v1/admin/projects/{project_id}/delete — soft delete. Serving
/// and deployments stop resolving; restore stays possible through the
/// team-level restore endpoint.
pub async fn remove(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.delete";
    let db = super::database(&state, OP)?;
    let project = load_project(db, project_id, OP).await?;
    let project = projects::soft_delete(db, project)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_audit(db, data.user_id, &project, "project.deleted").await;
    Ok(ok_response(json!({ "deleted": true })))
}
