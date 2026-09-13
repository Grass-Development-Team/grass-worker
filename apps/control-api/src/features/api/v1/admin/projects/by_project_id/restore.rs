use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::{
    domain::{deployments, projects, teams},
    infra::{
        database::entity::{deployment, project, team},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/restore",
        axum::routing::post(restore),
    )
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

/// POST /api/v1/admin/projects/{project_id}/restore
async fn restore(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.restore";
    let db = crate::infra::http::database(&state, OP)?;
    let project = load_project_any(db, project_id, OP).await?;
    if project.deleted_at.is_none() {
        return Err(AppError::Conflict {
            op: OP,
            message: "project is not deleted".to_owned(),
        });
    }
    let team = teams::get_by_id(db, project.team_id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .ok_or_else(|| AppError::NotFound {
            op: OP,
            message: "project team not found".to_owned(),
        })?;
    let cache = crate::infra::http::cache(&state, OP)?;
    let project = crate::domain::project_lifecycle::restore_project_with_quota(
        db,
        cache,
        OP,
        data.user_id,
        &team,
        project,
        crate::domain::project_lifecycle::RestoreAuditContext::PlatformAdmin,
    )
    .await?;
    Ok(ok_response(RestoreResponse {
        project: project_view(&project, Some(&team), None),
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
struct RestoreResponse {
    project: ProjectResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::project;
    use time::OffsetDateTime;
    use uuid::Uuid;

    use crate::infra::database::entity::TeamKind;
    use crate::infra::database::entity::team;
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

    #[tokio::test]
    async fn admin_restore_rejects_a_live_project_without_membership_queries() {
        use axum::extract::{Path, State};

        let actor_id = Uuid::now_v7();
        let project = inventory_project(Uuid::now_v7(), None, None);
        let project_id = project.id;
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[project]])
            .into_connection();
        let db_log = db.clone();
        let state = crate::state::ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).unwrap();
        let session = crate::infra::http::extractors::Session {
            data: grass_session::SessionData {
                auth_version: 1,
                user_id: actor_id,
                created_at: OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "admin-session".to_owned(),
        };

        let result = restore(State(state), session, Path(project_id)).await;

        assert!(matches!(
            result,
            Err(crate::infra::error::AppError::Conflict { .. })
        ));
        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(!statements.contains("team_members"), "{statements}");
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
    async fn admin_restore_reserves_and_commits_project_runtime_quota() {
        use axum::{
            body::to_bytes,
            extract::{Path, State},
            response::IntoResponse,
        };

        let actor_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let deleted_at = OffsetDateTime::from_unix_timestamp(10).unwrap();
        let project = inventory_project(team_id, None, Some(deleted_at));
        let project_id = project.id;
        let mut restored = project.clone();
        restored.deleted_at = None;
        let team = inventory_team(team_id);
        let plan = crate::infra::database::entity::quota_plan::Model {
            id: Uuid::now_v7(),
            code: "default".to_owned(),
            name: "Default".to_owned(),
            description: None,
            is_default: true,
            enabled: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let count_row = || {
            std::collections::BTreeMap::from([(
                "num_items".to_owned(),
                sea_orm::Value::BigInt(Some(0)),
            )])
        };
        let counter =
            |dimension: &str| crate::infra::database::entity::quota_usage_counter::Model {
                id: Uuid::now_v7(),
                team_id,
                dimension: dimension.to_owned(),
                used_value: 0,
                period_start: None,
                period_end: None,
                updated_at: OffsetDateTime::UNIX_EPOCH,
            };
        let quota_event = |dimension: &str| crate::infra::database::entity::quota_event::Model {
            id: Uuid::now_v7(),
            team_id,
            dimension: dimension.to_owned(),
            kind: crate::infra::database::entity::QuotaEventKind::Consume,
            delta_value: 1,
            idempotency_key: None,
            resource_type: Some("project".to_owned()),
            resource_id: Some(project_id),
            metadata: serde_json::json!({}),
            created_at: OffsetDateTime::UNIX_EPOCH,
        };
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[project.clone()]])
            .append_query_results([[team.clone()]])
            .append_query_results([[plan]])
            .append_query_results([
                Vec::<crate::infra::database::entity::quota_limit::Model>::new(),
            ])
            .append_query_results([[count_row()]])
            .append_query_results([[count_row()]])
            .append_query_results([[project.clone()]])
            .append_query_results([[restored.clone()]])
            .append_query_results([
                Vec::<crate::infra::database::entity::team_member::Model>::new(),
            ])
            .append_query_results([Vec::<crate::infra::database::entity::user::Model>::new()])
            .append_query_results([[quota_event("projects")]])
            .append_query_results([[counter("projects")]])
            .append_query_results([[counter("projects")]])
            .append_query_results([[quota_event("projects.static")]])
            .append_query_results([[counter("projects.static")]])
            .append_query_results([[counter("projects.static")]])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let db_log = db.clone();
        let state = crate::state::ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).unwrap();
        assert!(
            state
                .cache
                .set(grass_cache::CacheStore::Moka(
                    grass_cache::MokaCache::connect()
                ))
                .is_ok()
        );
        let session = crate::infra::http::extractors::Session {
            data: grass_session::SessionData {
                auth_version: 1,
                user_id: actor_id,
                created_at: OffsetDateTime::UNIX_EPOCH,
                last_accessed_at: OffsetDateTime::UNIX_EPOCH,
            },
            session_id: "admin-session".to_owned(),
        };

        let response = restore(State(state), session, Path(project_id))
            .await
            .expect("admin restore should reserve quota and restore the tombstone")
            .into_response();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["data"]["project"]["status"], "active");
        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(statements.contains("quota_events"), "{statements}");
        assert!(statements.contains("projects.static"), "{statements}");
    }
}
