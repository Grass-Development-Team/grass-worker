//! Database-backed project business functions.

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{ProjectRuntime, project};

pub struct CreateProjectParams {
    pub team_id: Uuid,
    pub created_by_user_id: Option<Uuid>,
    pub slug: String,
    pub name: String,
    pub runtime: ProjectRuntime,
    pub repository_url: Option<String>,
    pub default_branch: Option<String>,
    pub install_command: Option<String>,
    pub build_command: Option<String>,
    pub output_directory: Option<String>,
    pub source_config: serde_json::Value,
    pub build_config: serde_json::Value,
}

pub struct UpdateProjectParams {
    pub name: Option<String>,
    pub repository_url: Option<Option<String>>,
    pub default_branch: Option<Option<String>>,
    pub install_command: Option<Option<String>>,
    pub build_command: Option<Option<String>>,
    pub output_directory: Option<Option<String>>,
    pub source_config: Option<serde_json::Value>,
    pub build_config: Option<serde_json::Value>,
}

pub async fn create_project<C: ConnectionTrait>(
    db: &C,
    params: CreateProjectParams,
) -> anyhow::Result<project::Model> {
    let now = OffsetDateTime::now_utc();
    project::ActiveModel {
        id: Set(Uuid::now_v7()),
        team_id: Set(params.team_id),
        created_by_user_id: Set(params.created_by_user_id),
        slug: Set(params.slug),
        name: Set(params.name),
        runtime: Set(params.runtime),
        repository_url: Set(params.repository_url),
        default_branch: Set(params.default_branch),
        install_command: Set(params.install_command),
        build_command: Set(params.build_command),
        output_directory: Set(params.output_directory),
        source_config: Set(params.source_config),
        build_config: Set(params.build_config),
        archived_at: Set(None),
        deleted_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .map_err(Into::into)
}

/// Returns a project that has not been soft deleted. Archived projects are
/// still returned; callers decide whether archived projects are acceptable
/// for the operation.
pub async fn get_by_id<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> anyhow::Result<Option<project::Model>> {
    project::Entity::find()
        .filter(project::Column::Id.eq(project_id))
        .filter(project::Column::DeletedAt.is_null())
        .one(db)
        .await
        .map_err(Into::into)
}

/// Returns a project including soft-deleted rows, used by restore and hard
/// delete flows.
pub async fn get_by_id_any<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> anyhow::Result<Option<project::Model>> {
    project::Entity::find()
        .filter(project::Column::Id.eq(project_id))
        .one(db)
        .await
        .map_err(Into::into)
}

pub async fn list_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
) -> anyhow::Result<Vec<project::Model>> {
    project::Entity::find()
        .filter(project::Column::TeamId.eq(team_id))
        .filter(project::Column::DeletedAt.is_null())
        .order_by_desc(project::Column::CreatedAt)
        .all(db)
        .await
        .map_err(Into::into)
}

pub async fn update<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
    params: UpdateProjectParams,
) -> anyhow::Result<project::Model> {
    let mut active: project::ActiveModel = project.into();
    if let Some(name) = params.name {
        active.name = Set(name);
    }
    if let Some(repository_url) = params.repository_url {
        active.repository_url = Set(repository_url);
    }
    if let Some(default_branch) = params.default_branch {
        active.default_branch = Set(default_branch);
    }
    if let Some(install_command) = params.install_command {
        active.install_command = Set(install_command);
    }
    if let Some(build_command) = params.build_command {
        active.build_command = Set(build_command);
    }
    if let Some(output_directory) = params.output_directory {
        active.output_directory = Set(output_directory);
    }
    if let Some(source_config) = params.source_config {
        active.source_config = Set(source_config);
    }
    if let Some(build_config) = params.build_config {
        active.build_config = Set(build_config);
    }
    active.update(db).await.map_err(Into::into)
}

pub async fn set_archived<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
    archived: bool,
) -> anyhow::Result<project::Model> {
    let mut active: project::ActiveModel = project.into();
    active.archived_at = Set(archived.then(OffsetDateTime::now_utc));
    active.update(db).await.map_err(Into::into)
}

#[allow(dead_code)]
pub async fn soft_delete<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
) -> anyhow::Result<project::Model> {
    soft_delete_at(db, project, OffsetDateTime::now_utc()).await
}

pub async fn soft_delete_at<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
    deleted_at: OffsetDateTime,
) -> anyhow::Result<project::Model> {
    let mut active: project::ActiveModel = project.into();
    active.deleted_at = Set(Some(deleted_at));
    active.update(db).await.map_err(Into::into)
}

pub async fn restore<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
) -> anyhow::Result<project::Model> {
    let mut active: project::ActiveModel = project.into();
    active.deleted_at = Set(None);
    active.update(db).await.map_err(Into::into)
}

pub async fn transfer_team<C: ConnectionTrait>(
    db: &C,
    project: project::Model,
    new_team_id: Uuid,
) -> anyhow::Result<project::Model> {
    let mut active: project::ActiveModel = project.into();
    active.team_id = Set(new_team_id);
    active.update(db).await.map_err(Into::into)
}

pub async fn hard_delete<C: ConnectionTrait>(db: &C, project_id: Uuid) -> anyhow::Result<()> {
    project::Entity::delete_by_id(project_id).exec(db).await?;
    Ok(())
}

/// Validates that a project is in a state that accepts new deployments.
#[allow(dead_code)] // Wired by deployment creation in Milestone 5.
pub fn ensure_deployable(project: &project::Model) -> Result<(), ProjectStateError> {
    if project.archived_at.is_some() {
        return Err(ProjectStateError::Archived);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectStateError {
    #[error("project is archived")]
    Archived,
}

pub fn runtime_value(runtime: &ProjectRuntime) -> &'static str {
    match runtime {
        ProjectRuntime::Static => "static",
        ProjectRuntime::Ssr => "ssr",
        ProjectRuntime::Hybrid => "hybrid",
        ProjectRuntime::Serverless => "serverless",
        ProjectRuntime::Edge => "edge",
    }
}

/// First-stage projects can be created as `static` or `ssr`; other runtime
/// kinds only appear as build inspection results.
pub fn parse_creatable_runtime(value: &str) -> Option<ProjectRuntime> {
    match value.trim().to_ascii_lowercase().as_str() {
        "static" => Some(ProjectRuntime::Static),
        "ssr" => Some(ProjectRuntime::Ssr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_static_and_ssr_projects_can_be_created() {
        assert_eq!(
            parse_creatable_runtime("static"),
            Some(ProjectRuntime::Static)
        );
        assert_eq!(parse_creatable_runtime(" SSR "), Some(ProjectRuntime::Ssr));
        assert_eq!(parse_creatable_runtime("serverless"), None);
        assert_eq!(parse_creatable_runtime("edge"), None);
        assert_eq!(parse_creatable_runtime("hybrid"), None);
    }

    #[test]
    fn archived_projects_are_not_deployable() {
        let project = project::Model {
            id: Uuid::nil(),
            team_id: Uuid::nil(),
            created_by_user_id: None,
            slug: "demo".into(),
            name: "Demo".into(),
            runtime: ProjectRuntime::Static,
            repository_url: None,
            default_branch: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            source_config: serde_json::json!({}),
            build_config: serde_json::json!({}),
            archived_at: Some(OffsetDateTime::UNIX_EPOCH),
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        assert!(ensure_deployable(&project).is_err());

        let active = project::Model {
            archived_at: None,
            ..project
        };
        assert!(ensure_deployable(&active).is_ok());
    }

    #[tokio::test]
    async fn project_creation_persists_the_creator() {
        use sea_orm::{DbBackend, MockDatabase};

        let project_id = Uuid::now_v7();
        let team_id = Uuid::now_v7();
        let creator_user_id = Uuid::now_v7();
        let now = OffsetDateTime::UNIX_EPOCH;
        let expected = project::Model {
            id: project_id,
            team_id,
            created_by_user_id: Some(creator_user_id),
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
        let db = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[expected]])
            .into_connection();

        let created = create_project(
            &db,
            CreateProjectParams {
                team_id,
                created_by_user_id: Some(creator_user_id),
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
            },
        )
        .await
        .unwrap();

        assert_eq!(created.created_by_user_id, Some(creator_user_id));
        let statements = format!("{:?}", db.into_transaction_log());
        assert!(statements.contains("created_by_user_id"));
        assert!(statements.contains(&creator_user_id.to_string()));
    }
}
