use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, ConnectionTrait, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::http::timestamps::ts;
use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        deployments, hosts, notifications, projects,
    },
    infra::{
        database::entity::{
            AuditEventResult, HostBindingKind, HostBindingStatus, HostReviewStatus, audit_event,
            deployment, project, project_host_binding, team,
        },
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

#[derive(Deserialize)]
pub struct UpdateSlugRequest {
    pub slug: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ActivityQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

fn binding_audit_target_types() -> [&'static str; 2] {
    ["host", "project_host_binding"]
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

/// GET /api/v1/admin/projects/{project_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.detail";
    let db = super::database(&state, OP)?;
    let project = load_project(db, project_id, OP).await?;
    let team = team::Entity::find_by_id(project.team_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(json!({
        "project": {
            "id": project.id,
            "uuid": project.id,
            "slug": project.slug,
            "name": project.name,
            "runtime": projects::runtime_value(&project.runtime),
            "repository_url": project.repository_url,
            "default_branch": project.default_branch,
            "install_command": project.install_command,
            "build_command": project.build_command,
            "output_directory": project.output_directory,
            "source_config": project.source_config,
            "build_config": project.build_config,
            "archived_at": ts(project.archived_at),
            "created_at": ts(project.created_at),
            "updated_at": ts(project.updated_at),
        },
        "team": team.map(|team| json!({ "id": team.id, "slug": team.slug, "name": team.name })),
    })))
}

/// PATCH /api/v1/admin/projects/{project_id}/slug
pub async fn update_slug(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<UpdateSlugRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.slug.update";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
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
    Ok(ok_response(
        json!({ "project": { "id": updated.id, "uuid": updated.id, "slug": updated.slug }, "reason": reason }),
    ))
}

fn admin_deployment_view(item: &deployment::Model) -> serde_json::Value {
    json!({
        "id": item.id,
        "project_id": item.project_id,
        "environment": deployments::environment_value(&item.environment),
        "build_status": deployments::build_status_value(&item.build_status),
        "serve_status": deployments::serve_status_value(&item.serve_status),
        "release_status": deployments::release_status_value(&item.release_status),
        "release_pending": item.pending_release_reason.is_some(),
        "preview_host": item.preview_host,
        "source_repository_url": item.source_repository_url,
        "source_branch": item.source_branch,
        "commit_hash": item.commit_hash,
        "commit_message": item.commit_message,
        "build_stage": item.build_stage,
        "failure_code": item.failure_code,
        "failure_message": item.failure_message,
        "serve_failure_code": item.serve_failure_code,
        "serve_failure_message": item.serve_failure_message,
        "claimed_at": ts(item.claimed_at),
        "build_started_at": ts(item.build_started_at),
        "build_finished_at": ts(item.build_finished_at),
        "serve_started_at": ts(item.serve_started_at),
        "serve_finished_at": ts(item.serve_finished_at),
        "created_at": ts(item.created_at),
        "updated_at": ts(item.updated_at),
    })
}

/// GET /api/v1/admin/projects/{project_id}/deployments
pub async fn deployments(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.deployments";
    let db = super::database(&state, OP)?;
    let _project = load_project(db, project_id, OP).await?;
    let items = deployment::Entity::find()
        .filter(deployment::Column::ProjectId.eq(project_id))
        .filter(deployment::Column::DeletedAt.is_null())
        .order_by_desc(deployment::Column::CreatedAt)
        .order_by_desc(deployment::Column::Id)
        .all(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(
        json!({ "deployments": items.iter().map(admin_deployment_view).collect::<Vec<_>>() }),
    ))
}

fn admin_binding_view(binding: &project_host_binding::Model) -> serde_json::Value {
    json!({
        "id": binding.id,
        "project_id": binding.project_id,
        "host": binding.host,
        "kind": match binding.kind { HostBindingKind::Platform => "platform", HostBindingKind::Custom => "custom" },
        "environment": match binding.environment { crate::infra::database::entity::HostBindingEnvironment::Production => "production", crate::infra::database::entity::HostBindingEnvironment::Preview => "preview", crate::infra::database::entity::HostBindingEnvironment::All => "all" },
        "status": match binding.status { HostBindingStatus::Pending => "pending", HostBindingStatus::Active => "active", HostBindingStatus::Failed => "failed", HostBindingStatus::Disabled => "disabled" },
        "review_status": match binding.review_status { HostReviewStatus::NotRequired => "not_required", HostReviewStatus::Pending => "pending", HostReviewStatus::Approved => "approved", HostReviewStatus::Rejected => "rejected" },
        "failure_reason": binding.failure_reason,
        "is_primary": binding.is_primary,
        "host_source_id": binding.host_source_id,
        "reviewed_by_user_id": binding.reviewed_by_user_id,
        "reviewed_at": ts(binding.reviewed_at),
        "review_reason": binding.review_reason,
        "created_at": ts(binding.created_at),
        "updated_at": ts(binding.updated_at),
    })
}

/// GET /api/v1/admin/projects/{project_id}/domains
pub async fn domains(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.domains";
    let db = super::database(&state, OP)?;
    let _project = load_project(db, project_id, OP).await?;
    let bindings = hosts::list_bindings_for_project(db, project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(
        json!({ "domains": bindings.iter().map(admin_binding_view).collect::<Vec<_>>() }),
    ))
}

/// GET /api/v1/admin/projects/{project_id}/activity
pub async fn activity(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ActivityQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.activity";
    let db = super::database(&state, OP)?;
    let _project = load_project(db, project_id, OP).await?;
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
    Ok(ok_response(json!({
        "events": events.iter().map(crate::features::api::v1::admin::audit_events::event_view).collect::<Vec<_>>(),
        "pagination": { "page": page, "per_page": per_page, "total": total, "total_pages": total.div_ceil(per_page) },
    })))
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

async fn record_project_event<C: ConnectionTrait>(
    db: &C,
    actor: Uuid,
    project: &project::Model,
    action: &str,
    target_url: String,
) -> anyhow::Result<()> {
    audits::create_platform_audit_event(
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
    .await?;
    notifications::create_project_notification(
        db,
        notifications::CreateProjectNotification {
            project,
            actor_user_id: actor,
            action,
            reason: None,
            target_url,
        },
    )
    .await?;
    Ok(())
}

/// POST /api/v1/admin/projects/{project_id}/archive
pub async fn archive(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.archive";
    let db = super::database(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project(&transaction, project_id, OP).await?;
    if project.archived_at.is_some() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is already archived".to_owned(),
        });
    }
    let project = projects::set_archived(&transaction, project, true)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_event(
        &transaction,
        data.user_id,
        &project,
        "project.archived",
        format!("/projects/{}", project.id),
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
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project(&transaction, project_id, OP).await?;
    if project.archived_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is not archived".to_owned(),
        });
    }
    let project = projects::set_archived(&transaction, project, false)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_event(
        &transaction,
        data.user_id,
        &project,
        "project.unarchived",
        format!("/projects/{}", project.id),
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
    let cache = super::cache(&state, OP)?;
    let transaction = db
        .begin()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let project = load_project(&transaction, project_id, OP).await?;
    let (project, bindings) =
        crate::features::api::v1::projects::lifecycle::soft_delete_project_records(
            &transaction,
            project,
        )
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    record_project_event(
        &transaction,
        data.user_id,
        &project,
        "project.deleted",
        "/projects".to_owned(),
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
    crate::features::api::v1::projects::lifecycle::finalize_deleted_project_resources(
        db, cache, OP, &project, &bindings,
    )
    .await?;
    Ok(ok_response(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::infra::database::entity::{
        PlatformRole, ProjectRuntime, TeamMemberRole, UserStatus, project, team_member, user,
    };

    use super::{binding_audit_target_types, record_project_event};

    #[test]
    fn project_activity_includes_both_host_audit_target_names() {
        assert_eq!(
            binding_audit_target_types(),
            ["host", "project_host_binding"]
        );
    }

    #[tokio::test]
    async fn platform_lifecycle_actions_write_audit_and_recipient_notifications() {
        for (action, target_url) in [
            ("project.archived", "/projects/project-id"),
            ("project.unarchived", "/projects/project-id"),
            ("project.deleted", "/projects"),
        ] {
            let actor_id = Uuid::now_v7();
            let team_id = Uuid::now_v7();
            let project_id = Uuid::now_v7();
            let now = OffsetDateTime::UNIX_EPOCH;
            let actor = user::Model {
                id: actor_id,
                email: "owner@example.invalid".to_owned(),
                display_name: Some("Team Owner".to_owned()),
                status: UserStatus::Active,
                platform_role: PlatformRole::Admin,
                email_verified_at: Some(now),
                last_login_at: None,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            };
            let member = team_member::Model {
                id: Uuid::now_v7(),
                team_id,
                user_id: actor_id,
                role: TeamMemberRole::Owner,
                invited_by_user_id: None,
                joined_at: now,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            };
            let project = project::Model {
                id: project_id,
                team_id,
                created_by_user_id: Some(actor_id),
                slug: "demo".to_owned(),
                name: "Demo".to_owned(),
                runtime: ProjectRuntime::Static,
                repository_url: None,
                default_branch: None,
                install_command: None,
                build_command: None,
                output_directory: None,
                source_config: serde_json::json!({}),
                build_config: serde_json::json!({}),
                archived_at: None,
                deleted_at: None,
                created_at: now,
                updated_at: now,
            };
            let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
                .append_query_results([[member]])
                .append_query_results([[actor.clone()]])
                .append_query_results([[actor]])
                .append_exec_results([
                    sea_orm::MockExecResult {
                        last_insert_id: 0,
                        rows_affected: 1,
                    },
                    sea_orm::MockExecResult {
                        last_insert_id: 0,
                        rows_affected: 1,
                    },
                ])
                .into_connection();

            record_project_event(&db, actor_id, &project, action, target_url.to_owned())
                .await
                .unwrap();

            let statements = format!("{:?}", db.into_transaction_log());
            assert!(statements.contains("INSERT INTO \\\"audit_events\\\""));
            assert!(statements.contains("INSERT INTO \\\"user_notifications\\\""));
            assert!(statements.contains(action));
            assert!(statements.contains(target_url));
        }
    }
}
