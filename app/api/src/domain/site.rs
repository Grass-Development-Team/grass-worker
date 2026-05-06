use crate::domain::{
    host::{HostError, normalize_host},
    release::static_site_artifact,
};
use grass_worker_database::entities::{deployment, project, project_host_binding};
use grass_worker_database::repository::{
    DeploymentArtifactRepository, DeploymentRepository, ProjectHostBindingRepository,
    ProjectRepository, SeaOrmDeploymentArtifactRepository, SeaOrmDeploymentRepository,
    SeaOrmProjectHostBindingRepository, SeaOrmProjectRepository,
};
use sea_orm::{DatabaseConnection, DbErr};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiteService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSite {
    pub project_id: Uuid,
    pub project_slug: String,
    pub host: String,
    pub root_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteErrorKind {
    Validation,
    NotFound,
    Forbidden,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteError {
    kind: SiteErrorKind,
    message: String,
}

impl SiteError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: SiteErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: SiteErrorKind::NotFound,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: SiteErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: SiteErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: SiteErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &SiteErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<HostError> for SiteError {
    fn from(value: HostError) -> Self {
        match value.kind() {
            crate::domain::host::HostErrorKind::Validation => {
                SiteError::validation(value.message())
            }
            crate::domain::host::HostErrorKind::NotFound => SiteError::not_found(value.message()),
            crate::domain::host::HostErrorKind::Forbidden => SiteError::forbidden(value.message()),
            crate::domain::host::HostErrorKind::Conflict => SiteError::conflict(value.message()),
            crate::domain::host::HostErrorKind::Internal => SiteError::internal(value.message()),
        }
    }
}

fn map_db_error(error: DbErr) -> SiteError {
    tracing::error!(error = %error, "site database operation failed");
    SiteError::internal(error.to_string())
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

fn host_binding_repository(database: &DatabaseConnection) -> SeaOrmProjectHostBindingRepository {
    SeaOrmProjectHostBindingRepository::new(clone_database_connection(database))
}

fn deployment_repository(database: &DatabaseConnection) -> SeaOrmDeploymentRepository {
    SeaOrmDeploymentRepository::new(clone_database_connection(database))
}

fn deployment_artifact_repository(
    database: &DatabaseConnection,
) -> SeaOrmDeploymentArtifactRepository {
    SeaOrmDeploymentArtifactRepository::new(clone_database_connection(database))
}

impl SiteService {
    async fn resolve_active_site_for_project(
        &self,
        database: &DatabaseConnection,
        binding: project_host_binding::Model,
        project: project::Model,
    ) -> Result<Option<ResolvedSite>, SiteError> {
        if project.status == project::ProjectStatus::SoftDeleted {
            return Ok(None);
        }

        let active_deployment_id = match project.active_deployment_id {
            Some(active_deployment_id) => active_deployment_id,
            None => return Ok(None),
        };
        let deployment = match deployment_repository(database)
            .find_by_id(active_deployment_id)
            .await
            .map_err(map_db_error)?
        {
            Some(deployment)
                if deployment.project_id == project.id
                    && deployment.status == deployment::DeploymentStatus::Ready =>
            {
                deployment
            }
            Some(_) | None => return Ok(None),
        };
        let artifacts = deployment_artifact_repository(database)
            .list_by_deployment(deployment.id)
            .await
            .map_err(map_db_error)?;
        let artifact = match static_site_artifact(&artifacts) {
            Some(artifact) => artifact,
            None => return Ok(None),
        };

        Ok(Some(ResolvedSite {
            project_id: project.id,
            project_slug: project.slug,
            host: binding.host,
            root_dir: artifact.storage_path,
        }))
    }

    pub async fn resolve_by_host(
        &self,
        database: &DatabaseConnection,
        host: &str,
    ) -> Result<Option<ResolvedSite>, SiteError> {
        let host = normalize_host(host)?;
        let binding = match host_binding_repository(database)
            .find_by_host(&host)
            .await
            .map_err(map_db_error)?
        {
            Some(binding) => binding,
            None => return Ok(None),
        };
        let project = match project_repository(database)
            .find_by_id(binding.project_id)
            .await
            .map_err(map_db_error)?
        {
            Some(project) => project,
            None => return Ok(None),
        };

        self.resolve_active_site_for_project(database, binding, project)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use grass_worker_database::entities::deployment_artifact;
    use sea_orm::{DatabaseBackend, MockDatabase};

    fn sample_project(
        id: Uuid,
        active_deployment_id: Option<Uuid>,
        status: project::ProjectStatus,
    ) -> project::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id: Uuid::new_v4(),
            active_deployment_id,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            repository_url: None,
            production_branch: None,
            root_directory: None,
            install_command: None,
            build_command: None,
            output_directory: None,
            status,
            created_at: now,
            updated_at: now,
            archived_at: None,
            soft_deleted_at: None,
        }
    }

    fn sample_binding(project_id: Uuid) -> project_host_binding::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 5, 0).unwrap();
        project_host_binding::Model {
            id: Uuid::new_v4(),
            project_id,
            source_id: None,
            host: "docs.example.com".to_owned(),
            is_primary: true,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_deployment(id: Uuid, project_id: Uuid) -> deployment::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 10, 0).unwrap();
        deployment::Model {
            id,
            project_id,
            status: deployment::DeploymentStatus::Ready,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            last_stage: None,
            failure_message: None,
            created_at: now,
            started_at: Some(now),
            finished_at: Some(now),
        }
    }

    #[tokio::test]
    async fn resolve_by_host_returns_active_ready_static_site() {
        let project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_binding(project_id)]])
            .append_query_results([[sample_project(
                project_id,
                Some(deployment_id),
                project::ProjectStatus::Active,
            )]])
            .append_query_results([[sample_deployment(deployment_id, project_id)]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::new_v4(),
                deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/tmp/docs-site".to_owned(),
                checksum_sha256: Some("abc123".to_owned()),
                size_bytes: Some(1024),
                created_at: Utc.with_ymd_and_hms(2026, 5, 3, 10, 15, 0).unwrap(),
            }]])
            .into_connection();

        let resolved = SiteService
            .resolve_by_host(&database, "Docs.EXAMPLE.com:443.")
            .await
            .unwrap();

        assert_eq!(
            resolved,
            Some(ResolvedSite {
                project_id,
                project_slug: "docs-site".to_owned(),
                host: "docs.example.com".to_owned(),
                root_dir: "/tmp/docs-site".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn resolve_by_host_returns_archived_project_site_when_deployment_ready() {
        let project_id = Uuid::new_v4();
        let deployment_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_binding(project_id)]])
            .append_query_results([[sample_project(
                project_id,
                Some(deployment_id),
                project::ProjectStatus::Archived,
            )]])
            .append_query_results([[sample_deployment(deployment_id, project_id)]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::new_v4(),
                deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/tmp/docs-site".to_owned(),
                checksum_sha256: Some("abc123".to_owned()),
                size_bytes: Some(1024),
                created_at: Utc.with_ymd_and_hms(2026, 5, 3, 10, 15, 0).unwrap(),
            }]])
            .into_connection();

        let resolved = SiteService
            .resolve_by_host(&database, "docs.example.com")
            .await
            .unwrap();

        assert_eq!(
            resolved.as_ref().map(|site| site.root_dir.as_str()),
            Some("/tmp/docs-site")
        );
    }
}
