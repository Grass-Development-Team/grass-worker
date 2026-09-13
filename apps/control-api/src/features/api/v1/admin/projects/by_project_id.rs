pub(crate) mod activity;
pub(crate) mod archive;
pub(crate) mod delete;
pub(crate) mod deployments;
pub(crate) mod domains;
pub(crate) mod restore;
pub(crate) mod slug;
pub(crate) mod unarchive;

use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use sea_orm::{ConnectionTrait, EntityTrait};
use uuid::Uuid;

use crate::{
    domain::projects,
    infra::{
        database::entity::{project, team},
        error::{AppError, ok_response},
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/projects/{project_id}", axum::routing::get(detail))
        .merge(activity::router())
        .merge(archive::router())
        .merge(delete::router())
        .merge(deployments::router())
        .merge(domains::router())
        .merge(restore::router())
        .merge(slug::router())
        .merge(unarchive::router())
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

/// GET /api/v1/admin/projects/{project_id}
pub async fn detail(
    State(state): State<ControlApiState>,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "admin.projects.detail";
    let db = crate::infra::http::database(&state, OP)?;
    let project = load_project_any(db, project_id, OP).await?;
    let team = team::Entity::find_by_id(project.team_id)
        .one(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(DetailResponse {
        project: DetailProjectResponse {
            id: project.id,
            uuid: project.id,
            slug: project.slug.clone(),
            name: project.name.clone(),
            runtime: projects::runtime_value(&project.runtime),
            repository_url: project.repository_url.clone(),
            default_branch: project.default_branch.clone(),
            install_command: project.install_command.clone(),
            build_command: project.build_command.clone(),
            output_directory: project.output_directory.clone(),
            source_config: project.source_config.clone(),
            build_config: project.build_config.clone(),
            archived_at: project.archived_at,
            deleted_at: project.deleted_at,
            status: project_status(&project),
            created_at: project.created_at,
            updated_at: project.updated_at,
        },
        team: team.map(|team| DetailTeamResponse {
            id: team.id,
            slug: team.slug.clone(),
            name: team.name.clone(),
        }),
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
struct DetailProjectResponse {
    id: uuid::Uuid,
    uuid: uuid::Uuid,
    slug: String,
    name: String,
    runtime: &'static str,
    repository_url: Option<String>,
    default_branch: Option<String>,
    install_command: Option<String>,
    build_command: Option<String>,
    output_directory: Option<String>,
    source_config: serde_json::Value,
    build_config: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    archived_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    deleted_at: Option<time::OffsetDateTime>,
    status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: time::OffsetDateTime,
}

#[derive(serde::Serialize)]
struct DetailTeamResponse {
    id: uuid::Uuid,
    slug: String,
    name: String,
}

#[derive(serde::Serialize)]
struct DetailResponse {
    project: DetailProjectResponse,
    team: Option<DetailTeamResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::ProjectRuntime;
    use crate::infra::database::entity::TeamKind;
    use crate::infra::database::entity::project;
    use crate::infra::database::entity::team;
    use time::OffsetDateTime;
    use uuid::Uuid;

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
    async fn admin_detail_loads_and_describes_a_deleted_project() {
        use axum::{
            body::to_bytes,
            extract::{Path, State},
            response::IntoResponse,
        };

        let team_id = Uuid::now_v7();
        let project = inventory_project(
            team_id,
            None,
            Some(OffsetDateTime::from_unix_timestamp(10).unwrap()),
        );
        let project_id = project.id;
        let db = sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[project.clone()]])
            .append_query_results([[inventory_team(team_id)]])
            .into_connection();
        let db_log = db.clone();
        let state = crate::state::ControlApiState::new(
            crate::infra::config::ControlApiConfig::default(),
            "unused.toml",
        );
        state.database.set(db).unwrap();

        let response = detail(State(state), Path(project_id))
            .await
            .unwrap()
            .into_response();
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["data"]["project"]["status"], "deleted");
        assert_eq!(
            body["data"]["project"]["deleted_at"],
            "1970-01-01T00:00:10Z"
        );
        let statements = format!("{:?}", db_log.into_transaction_log());
        assert!(
            !statements.contains("deleted_at\\\" IS NULL"),
            "{statements}"
        );
    }
}
