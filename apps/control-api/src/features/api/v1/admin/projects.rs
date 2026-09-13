pub(crate) mod batch;
pub(crate) mod by_project_id;

use axum::{
    extract::{Query, State},
    response::IntoResponse,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    domain::{deployments, projects},
    infra::{
        database::entity::{deployment, project, team},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/projects", axum::routing::get(list))
        .merge(batch::router())
        .merge(by_project_id::router())
}

fn deployment_summary(deployment: &deployment::Model) -> DeploymentSummaryResponse {
    DeploymentSummaryResponse {
        id: deployment.id,
        environment: deployments::environment_value(&deployment.environment),
        build_status: deployments::build_status_value(&deployment.build_status),
        release_status: deployments::release_status_value(&deployment.release_status),
        created_at: deployment.created_at,
    }
}

fn project_view(
    project: &project::Model,
    team: Option<&team::Model>,
    latest: Option<&deployment::Model>,
) -> ProjectResponse {
    ProjectResponse {
        id: project.id,
        slug: project.slug.clone(),
        name: project.name.clone(),
        runtime: projects::runtime_value(&project.runtime),
        repository_url: project.repository_url.clone(),
        team: team.map(|team| ProjectTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
        }),
        latest_deployment: latest.map(deployment_summary),
        archived_at: project.archived_at,
        deleted_at: project.deleted_at,
        status: project_status(project),
        created_at: project.created_at,
    }
}

fn project_status(project: &project::Model) -> &'static str {
    if project.deleted_at.is_some() {
        "deleted"
    } else if project.archived_at.is_some() {
        "archived"
    } else {
        "active"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectStatusFilter {
    Active,
    Archived,
    Deleted,
}

fn parse_project_status(
    value: Option<&str>,
    op: &'static str,
) -> Result<Option<ProjectStatusFilter>, AppError> {
    value
        .map(|value| match value {
            "active" => Ok(ProjectStatusFilter::Active),
            "archived" => Ok(ProjectStatusFilter::Archived),
            "deleted" => Ok(ProjectStatusFilter::Deleted),
            _ => Err(AppError::Validation {
                op,
                message: "status must be active, archived, or deleted".to_owned(),
            }),
        })
        .transpose()
}

#[derive(Deserialize)]
pub struct ListProjectsQuery {
    pub q: Option<String>,
    pub limit: Option<u64>,
    pub status: Option<String>,
}

/// GET /api/v1/admin/projects — every project on the platform
/// with its team and most recent deployment.
pub async fn list(
    State(state): State<ControlApiState>,
    Query(query): Query<ListProjectsQuery>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.list";
    let status = parse_project_status(query.status.as_deref(), OP)?;
    let db = crate::infra::http::database(&state, OP)?;

    let mut select = project::Entity::find();
    if let Some(status) = status {
        select = match status {
            ProjectStatusFilter::Active => select
                .filter(project::Column::DeletedAt.is_null())
                .filter(project::Column::ArchivedAt.is_null()),
            ProjectStatusFilter::Archived => select
                .filter(project::Column::DeletedAt.is_null())
                .filter(project::Column::ArchivedAt.is_not_null()),
            ProjectStatusFilter::Deleted => select.filter(project::Column::DeletedAt.is_not_null()),
        };
    }
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

    Ok(ok_response(ListResponse {
        projects: projects_list
            .iter()
            .map(|project| {
                project_view(
                    project,
                    teams.get(&project.team_id),
                    latest.get(&project.id),
                )
            })
            .collect::<Vec<_>>(),
    }))
}

#[derive(serde::Serialize)]
struct DeploymentSummaryResponse {
    id: uuid::Uuid,
    environment: &'static str,
    build_status: &'static str,
    release_status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ProjectTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

#[derive(serde::Serialize)]
struct ProjectResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    team: Option<ProjectTeamResponse>,
    latest_deployment: Option<DeploymentSummaryResponse>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct ListResponse {
    projects: Vec<ProjectResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::notifications,
        infra::{
            audit::{self as audits, CreateAuditEventParams},
            database::entity::AuditEventResult,
        },
    };

    use crate::infra::database::entity::PlatformRole;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::TeamKind;
    use crate::infra::database::entity::TeamMemberRole;
    use crate::infra::database::entity::UserStatus;
    use crate::infra::database::entity::project;
    use crate::infra::database::entity::team;
    use crate::infra::database::entity::team_member;
    use crate::infra::database::entity::user;
    use serde_json::json;
    use time::OffsetDateTime;
    use uuid::Uuid;
    async fn record_project_event<C: crate::infra::audit::AuditConnection>(
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

    #[test]
    fn invalid_project_status_is_validation() {
        let error = parse_project_status(Some("purged"), "admin.projects.list").unwrap_err();
        assert!(matches!(
            error,
            crate::infra::error::AppError::Validation { .. }
        ));
    }

    fn inventory_project(
        team_id: Uuid,
        archived_at: Option<OffsetDateTime>,
        deleted_at: Option<OffsetDateTime>,
    ) -> project::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        project::Model {
            id: Uuid::now_v7(),
            team_id,
            created_by_user_id: None,
            slug: Uuid::now_v7().to_string(),
            name: "Inventory project".to_owned(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at,
            deleted_at,
            created_at: now,
            updated_at: now,
        }
    }

    fn inventory_team(team_id: Uuid) -> team::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        team::Model {
            id: team_id,
            slug: "inventory-team".to_owned(),
            name: "Inventory team".to_owned(),
            avatar_version: None,
            kind: TeamKind::Personal,
            group_id: None,
            explicit_quota_plan_id: None,
            owner_user_id: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn inventory_defaults_to_active_archived_and_deleted_projects() {
        use axum::{
            body::to_bytes,
            extract::{Query, State},
            response::IntoResponse,
        };

        let team_id = Uuid::now_v7();
        let deleted_at = OffsetDateTime::from_unix_timestamp(10).unwrap();
        let archived_at = OffsetDateTime::from_unix_timestamp(20).unwrap();
        let projects = vec![
            inventory_project(team_id, None, None),
            inventory_project(team_id, Some(archived_at), None),
            inventory_project(team_id, None, Some(deleted_at)),
        ];
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([projects])
            .append_query_results([[inventory_team(team_id)]])
            .append_query_results([Vec::<crate::infra::database::entity::deployment::Model>::new()])
            .into_connection();
        let mut config = crate::infra::config::ControlApiConfig::default();
        config.redis.backend = grass_cache::CacheBackend::Moka;
        let state = crate::state::ControlApiState::new(config, "unused.toml");
        state.database.set(db).unwrap();

        let response = list(
            State(state),
            Query(ListProjectsQuery {
                q: None,
                limit: None,
                status: None,
            }),
        )
        .await
        .unwrap()
        .into_response();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        let rows = body["data"]["projects"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["status"], "active");
        assert_eq!(rows[1]["status"], "archived");
        assert_eq!(rows[2]["status"], "deleted");
        assert_eq!(rows[2]["deleted_at"], "1970-01-01T00:00:10Z");
    }

    #[tokio::test]
    async fn inventory_status_filter_adds_a_typed_deleted_predicate() {
        use axum::{
            extract::{Query, State},
            response::IntoResponse,
        };

        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([Vec::<project::Model>::new()])
            .into_connection();
        let db_log = db.clone();
        let state = crate::state::ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).unwrap();

        let _ = list(
            State(state),
            Query(ListProjectsQuery {
                q: None,
                limit: Some(10),
                status: Some("deleted".to_owned()),
            }),
        )
        .await
        .unwrap()
        .into_response();
        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(
            statements.contains("deleted_at\\\" IS NOT NULL"),
            "{statements}"
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
                auth_version: 1,
                id: actor_id,
                email: "owner@example.invalid".to_owned(),
                display_name: Some("Team Owner".to_owned()),
                avatar_version: None,
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
