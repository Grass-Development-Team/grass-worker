use crate::domain::{
    auth::AuthenticatedUser,
    project::{self, enforce_project_visibility},
};
use chrono::Utc;
use grass_worker_database::entities::{deployment, project as project_entity};
use grass_worker_database::repository::{
    DeploymentRepository, NewDeployment, ProjectRepository, SeaOrmDeploymentRepository,
    SeaOrmProjectRepository,
};
use sea_orm::{DatabaseConnection, DbErr};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeploymentService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentErrorKind {
    Validation,
    NotFound,
    Forbidden,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentError {
    kind: DeploymentErrorKind,
    message: String,
}

impl DeploymentError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: DeploymentErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: DeploymentErrorKind::NotFound,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: DeploymentErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: DeploymentErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: DeploymentErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &DeploymentErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

fn map_db_error(error: DbErr) -> DeploymentError {
    tracing::error!(error = %error, "deployment database operation failed");
    DeploymentError::internal(error.to_string())
}

fn map_project_error(error: project::ProjectError) -> DeploymentError {
    match error.kind() {
        project::ProjectErrorKind::Validation => DeploymentError::validation(error.message()),
        project::ProjectErrorKind::NotFound => DeploymentError::not_found(error.message()),
        project::ProjectErrorKind::Forbidden => DeploymentError::forbidden(error.message()),
        project::ProjectErrorKind::Conflict => DeploymentError::conflict(error.message()),
        project::ProjectErrorKind::Internal => DeploymentError::internal(error.message()),
    }
}

fn is_project_fk_conflict(error: &DbErr) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("fk-deployments-project-id")
        || (message.contains("foreign key")
            && message.contains("deployment")
            && message.contains("project"))
}

fn normalize_optional_field(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn clone_database_connection(database: &DatabaseConnection) -> DatabaseConnection {
    match database {
        DatabaseConnection::SqlxPostgresPoolConnection(connection) => {
            DatabaseConnection::SqlxPostgresPoolConnection(connection.clone())
        }
        DatabaseConnection::MockDatabaseConnection(connection) => {
            DatabaseConnection::MockDatabaseConnection(connection.clone())
        }
        DatabaseConnection::Disconnected => DatabaseConnection::Disconnected,
    }
}

fn project_repository(database: &DatabaseConnection) -> SeaOrmProjectRepository {
    SeaOrmProjectRepository::new(clone_database_connection(database))
}

fn deployment_repository(database: &DatabaseConnection) -> SeaOrmDeploymentRepository {
    SeaOrmDeploymentRepository::new(clone_database_connection(database))
}

impl DeploymentService {
    async fn load_visible_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<project_entity::Model, DeploymentError> {
        let project = project_repository(database)
            .find_by_id(project_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| DeploymentError::not_found("project not found"))?;

        enforce_project_visibility(actor, project).map_err(map_project_error)
    }

    pub async fn create(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        source_branch: Option<&str>,
        source_revision: Option<&str>,
    ) -> Result<deployment::Model, DeploymentError> {
        let project = self.load_visible_project(database, actor, project_id).await?;
        if project.status != project_entity::ProjectStatus::Active {
            return Err(DeploymentError::conflict("project is not active"));
        }

        deployment_repository(database)
            .create(NewDeployment {
                id: Uuid::new_v4(),
                project_id,
                source_branch: normalize_optional_field(source_branch),
                source_revision: normalize_optional_field(source_revision),
                created_at: Utc::now(),
            })
            .await
            .map_err(|error| {
                if is_project_fk_conflict(&error) {
                    DeploymentError::not_found("project not found")
                } else {
                    map_db_error(error)
                }
            })
    }

    pub async fn list_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<Vec<deployment::Model>, DeploymentError> {
        let _project = self.load_visible_project(database, actor, project_id).await?;
        deployment_repository(database)
            .list_by_project(project_id)
            .await
            .map_err(map_db_error)
    }

    pub async fn get_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<deployment::Model, DeploymentError> {
        let _project = self.load_visible_project(database, actor, project_id).await?;
        let deployment = deployment_repository(database)
            .find_by_id(deployment_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| DeploymentError::not_found("deployment not found"))?;

        if deployment.project_id != project_id {
            return Err(DeploymentError::not_found("deployment not found"));
        }

        Ok(deployment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};
    use grass_worker_database::entities::{deployment, project};
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};
    use uuid::Uuid;

    fn actor(id: Uuid, is_admin: bool) -> AuthenticatedUser {
        AuthenticatedUser {
            id,
            email: if is_admin {
                "admin@example.com".to_owned()
            } else {
                "owner@example.com".to_owned()
            },
            is_admin,
            is_initial_admin: is_admin,
        }
    }

    fn sample_project(
        id: Uuid,
        owner_user_id: Uuid,
        status: project::ProjectStatus,
    ) -> project::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 8, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: status.clone(),
            created_at,
            updated_at: created_at,
            archived_at: if status == project::ProjectStatus::Archived {
                Some(created_at + Duration::hours(1))
            } else {
                None
            },
            soft_deleted_at: if status == project::ProjectStatus::SoftDeleted {
                Some(created_at + Duration::hours(2))
            } else {
                None
            },
        }
    }

    fn sample_deployment(id: Uuid, project_id: Uuid) -> deployment::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 28, 9, 0, 0).unwrap();
        deployment::Model {
            id,
            project_id,
            status: deployment::DeploymentStatus::Pending,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            created_at,
            started_at: None,
            finished_at: None,
        }
    }

    #[tokio::test]
    async fn create_rejects_archived_projects() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Archived,
            )]])
            .into_connection();

        let error = DeploymentService
            .create(
                &database,
                &actor(owner_id, false),
                project_id,
                Some("main"),
                Some("deadbeef"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::Conflict);
        assert_eq!(error.message(), "project is not active");
    }

    #[tokio::test]
    async fn create_normalizes_blank_source_fields_to_none() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Active,
            )]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();

        let deployment = DeploymentService
            .create(
                &database,
                &actor(owner_id, false),
                project_id,
                Some("   "),
                Some(""),
            )
            .await
            .unwrap();

        assert_eq!(deployment.status, deployment::DeploymentStatus::Pending);
        assert_eq!(deployment.source_branch, None);
        assert_eq!(deployment.source_revision, None);
    }

    #[tokio::test]
    async fn create_rejects_soft_deleted_projects_for_admin() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::SoftDeleted,
            )]])
            .into_connection();

        let error = DeploymentService
            .create(
                &database,
                &actor(Uuid::new_v4(), true),
                project_id,
                Some("main"),
                Some("deadbeef"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::Conflict);
        assert_eq!(error.message(), "project is not active");
    }

    #[tokio::test]
    async fn create_maps_project_fk_conflict_to_not_found() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Active,
            )]])
            .append_exec_errors([DbErr::Custom(
                "Foreign Key Constraint Violated: insert or update on table \"deployments\" violates foreign key constraint \"fk-deployments-project-id\""
                    .to_owned(),
            )])
            .into_connection();

        let error = DeploymentService
            .create(
                &database,
                &actor(owner_id, false),
                project_id,
                Some("main"),
                Some("deadbeef"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::NotFound);
        assert_eq!(error.message(), "project not found");
    }

    #[tokio::test]
    async fn list_for_project_returns_visible_project_deployments() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let first = sample_deployment(Uuid::new_v4(), project_id);
        let second = sample_deployment(Uuid::new_v4(), project_id);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([vec![first.clone(), second.clone()]])
            .into_connection();

        let deployments = DeploymentService
            .list_for_project(&database, &actor(owner_id, false), project_id)
            .await
            .unwrap();

        assert_eq!(deployments, vec![first, second]);
    }

    #[tokio::test]
    async fn list_for_project_rejects_non_admin_soft_deleted_owned_project() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::SoftDeleted,
            )]])
            .into_connection();

        let error = DeploymentService
            .list_for_project(&database, &actor(owner_id, false), project_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::NotFound);
        assert_eq!(error.message(), "project not found");
    }

    #[tokio::test]
    async fn get_for_project_rejects_deployments_from_other_projects() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let other_project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment(deployment_id, other_project_id)]])
            .into_connection();

        let error = DeploymentService
            .get_for_project(&database, &actor(owner_id, false), project_id, deployment_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::NotFound);
        assert_eq!(error.message(), "deployment not found");
    }
}
