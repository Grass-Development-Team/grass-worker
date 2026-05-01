use crate::domain::{
    auth::AuthenticatedUser,
    project::{self, enforce_project_visibility},
};
use chrono::Utc;
use grass_worker_database::entities::{deployment, deployment_artifact, project as project_entity};
use grass_worker_database::repository::{
    DeploymentArtifactRepository, DeploymentRepository, NewDeployment, NewDeploymentArtifact,
    ProjectRepository, SeaOrmDeploymentArtifactRepository, SeaOrmDeploymentRepository,
    SeaOrmProjectRepository,
};
use sea_orm::{DatabaseConnection, DbErr};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeploymentService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterDeploymentArtifactInput {
    pub kind: deployment_artifact::ArtifactKind,
    pub storage_path: String,
    pub checksum_sha256: Option<String>,
    pub size_bytes: Option<i64>,
}

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

fn deployment_artifact_repository(
    database: &DatabaseConnection,
) -> SeaOrmDeploymentArtifactRepository {
    SeaOrmDeploymentArtifactRepository::new(clone_database_connection(database))
}

fn transition_plan(
    current_status: &deployment::DeploymentStatus,
    next_status: &deployment::DeploymentStatus,
) -> Result<
    (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ),
    DeploymentError,
> {
    let now = Utc::now();

    match (current_status, next_status) {
        (deployment::DeploymentStatus::Pending, deployment::DeploymentStatus::Processing) => {
            Ok((Some(now), None))
        }
        (deployment::DeploymentStatus::Pending, deployment::DeploymentStatus::Canceled) => {
            Ok((None, Some(now)))
        }
        (deployment::DeploymentStatus::Processing, deployment::DeploymentStatus::Ready)
        | (deployment::DeploymentStatus::Processing, deployment::DeploymentStatus::Failed)
        | (deployment::DeploymentStatus::Processing, deployment::DeploymentStatus::Canceled) => {
            Ok((None, Some(now)))
        }
        (deployment::DeploymentStatus::Pending, deployment::DeploymentStatus::Ready)
        | (deployment::DeploymentStatus::Pending, deployment::DeploymentStatus::Failed) => Err(
            DeploymentError::conflict("deployment must be processing before it can finish"),
        ),
        (_, deployment::DeploymentStatus::Pending) => Err(DeploymentError::conflict(
            "deployment cannot transition back to pending",
        )),
        (current, next) if current == next => Err(DeploymentError::conflict(format!(
            "deployment is already {}",
            deployment_status_label(current)
        ))),
        _ => Err(DeploymentError::conflict("deployment is already finished")),
    }
}

fn deployment_status_label(status: &deployment::DeploymentStatus) -> &'static str {
    match status {
        deployment::DeploymentStatus::Pending => "pending",
        deployment::DeploymentStatus::Processing => "processing",
        deployment::DeploymentStatus::Ready => "ready",
        deployment::DeploymentStatus::Failed => "failed",
        deployment::DeploymentStatus::Canceled => "canceled",
    }
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
        let project = self
            .load_visible_project(database, actor, project_id)
            .await?;
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
        let _project = self
            .load_visible_project(database, actor, project_id)
            .await?;
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
        let _project = self
            .load_visible_project(database, actor, project_id)
            .await?;
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

    pub async fn transition_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        deployment_id: Uuid,
        next_status: deployment::DeploymentStatus,
    ) -> Result<deployment::Model, DeploymentError> {
        let deployment = self
            .get_for_project(database, actor, project_id, deployment_id)
            .await?;
        let (started_at, finished_at) = transition_plan(&deployment.status, &next_status)?;

        deployment_repository(database)
            .set_status_if_current(
                deployment_id,
                deployment.status,
                next_status,
                started_at,
                finished_at,
            )
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| DeploymentError::conflict("deployment status changed during transition"))
    }

    pub async fn list_artifacts_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<Vec<deployment_artifact::Model>, DeploymentError> {
        let _deployment = self
            .get_for_project(database, actor, project_id, deployment_id)
            .await?;

        deployment_artifact_repository(database)
            .list_by_deployment(deployment_id)
            .await
            .map_err(map_db_error)
    }

    pub async fn register_artifact_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        deployment_id: Uuid,
        input: RegisterDeploymentArtifactInput,
    ) -> Result<deployment_artifact::Model, DeploymentError> {
        let deployment = self
            .get_for_project(database, actor, project_id, deployment_id)
            .await?;

        if deployment.status == deployment::DeploymentStatus::Pending {
            return Err(DeploymentError::conflict("deployment has not started"));
        }

        let storage_path = normalize_optional_field(Some(input.storage_path.as_str()))
            .ok_or_else(|| DeploymentError::validation("storage_path is required"))?;
        if input.size_bytes.is_some_and(|value| value < 0) {
            return Err(DeploymentError::validation(
                "size_bytes must be greater than or equal to 0",
            ));
        }

        deployment_artifact_repository(database)
            .create(NewDeploymentArtifact {
                id: Uuid::new_v4(),
                deployment_id,
                kind: input.kind,
                storage_path,
                checksum_sha256: normalize_optional_field(input.checksum_sha256.as_deref()),
                size_bytes: input.size_bytes,
                created_at: Utc::now(),
            })
            .await
            .map_err(map_db_error)
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
            active_deployment_id: None,
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
            .get_for_project(
                &database,
                &actor(owner_id, false),
                project_id,
                deployment_id,
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &DeploymentErrorKind::NotFound);
        assert_eq!(error.message(), "deployment not found");
    }
}
