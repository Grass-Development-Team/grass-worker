use crate::domain::{
    auth::AuthenticatedUser,
    project::{self, enforce_project_visibility},
};
use chrono::Utc;
use grass_worker_database::entities::{deployment, deployment_artifact, project as project_entity};
use grass_worker_database::repository::{
    DeploymentArtifactRepository, DeploymentRepository, ProjectHostBindingRepository,
    ProjectRepository, SeaOrmDeploymentArtifactRepository, SeaOrmDeploymentRepository,
    SeaOrmProjectHostBindingRepository, SeaOrmProjectRepository,
};
use sea_orm::{DatabaseConnection, DbErr};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseState {
    pub project_id: Uuid,
    pub project_slug: String,
    pub primary_host: Option<String>,
    pub active_deployment_id: Option<Uuid>,
    pub active_deployment: Option<deployment::Model>,
    pub rollback_deployment_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSiteRelease {
    pub project_slug: String,
    pub root_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseErrorKind {
    Validation,
    NotFound,
    Forbidden,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseError {
    kind: ReleaseErrorKind,
    message: String,
}

impl ReleaseError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::NotFound,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ReleaseErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &ReleaseErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

fn map_db_error(error: DbErr) -> ReleaseError {
    tracing::error!(error = %error, "release database operation failed");
    ReleaseError::internal(error.to_string())
}

fn map_project_error(error: project::ProjectError) -> ReleaseError {
    match error.kind() {
        project::ProjectErrorKind::Validation => ReleaseError::validation(error.message()),
        project::ProjectErrorKind::NotFound => ReleaseError::not_found(error.message()),
        project::ProjectErrorKind::Forbidden => ReleaseError::forbidden(error.message()),
        project::ProjectErrorKind::Conflict => ReleaseError::conflict(error.message()),
        project::ProjectErrorKind::Internal => ReleaseError::internal(error.message()),
    }
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

fn host_binding_repository(database: &DatabaseConnection) -> SeaOrmProjectHostBindingRepository {
    SeaOrmProjectHostBindingRepository::new(clone_database_connection(database))
}

pub(crate) fn static_site_artifact(
    artifacts: &[deployment_artifact::Model],
) -> Option<deployment_artifact::Model> {
    artifacts
        .iter()
        .find(|artifact| artifact.kind == deployment_artifact::ArtifactKind::StaticSite)
        .cloned()
}

fn releaseable_project(
    project: project_entity::Model,
) -> Result<project_entity::Model, ReleaseError> {
    if project.status == project_entity::ProjectStatus::SoftDeleted {
        return Err(ReleaseError::conflict("project is soft deleted"));
    }

    Ok(project)
}

fn rank_previous_release(
    left: &deployment::Model,
    right: &deployment::Model,
) -> std::cmp::Ordering {
    right
        .finished_at
        .cmp(&left.finished_at)
        .then_with(|| right.created_at.cmp(&left.created_at))
        .then_with(|| right.id.cmp(&left.id))
}

impl ReleaseService {
    async fn load_visible_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<project_entity::Model, ReleaseError> {
        let project = project_repository(database)
            .find_by_id(project_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ReleaseError::not_found("project not found"))?;

        enforce_project_visibility(actor, project).map_err(map_project_error)
    }

    async fn load_project_deployment(
        &self,
        database: &DatabaseConnection,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<deployment::Model, ReleaseError> {
        let deployment = deployment_repository(database)
            .find_by_id(deployment_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ReleaseError::not_found("deployment not found"))?;

        if deployment.project_id != project_id {
            return Err(ReleaseError::not_found("deployment not found"));
        }

        Ok(deployment)
    }

    async fn rollback_candidate_for_project(
        &self,
        database: &DatabaseConnection,
        project_id: Uuid,
        current_active_deployment_id: Uuid,
    ) -> Result<Option<deployment::Model>, ReleaseError> {
        let mut deployments = deployment_repository(database)
            .list_by_project(project_id)
            .await
            .map_err(map_db_error)?;
        deployments.retain(|deployment| {
            deployment.id != current_active_deployment_id
                && deployment.status == deployment::DeploymentStatus::Ready
        });
        deployments.sort_by(rank_previous_release);

        for deployment in deployments {
            let artifacts = deployment_artifact_repository(database)
                .list_by_deployment(deployment.id)
                .await
                .map_err(map_db_error)?;
            if static_site_artifact(&artifacts).is_some() {
                return Ok(Some(deployment));
            }
        }

        Ok(None)
    }

    async fn build_state(
        &self,
        database: &DatabaseConnection,
        project: &project_entity::Model,
    ) -> Result<ReleaseState, ReleaseError> {
        let primary_host = host_binding_repository(database)
            .find_primary_by_project(project.id)
            .await
            .map_err(map_db_error)?
            .map(|binding| binding.host);
        let active_deployment = match project.active_deployment_id {
            Some(active_deployment_id) => deployment_repository(database)
                .find_by_id(active_deployment_id)
                .await
                .map_err(map_db_error)?
                .filter(|deployment| deployment.project_id == project.id),
            None => None,
        };
        let rollback_deployment_id = match active_deployment.as_ref() {
            Some(active_deployment) => self
                .rollback_candidate_for_project(database, project.id, active_deployment.id)
                .await?
                .map(|deployment| deployment.id),
            None => None,
        };

        Ok(ReleaseState {
            project_id: project.id,
            project_slug: project.slug.clone(),
            primary_host,
            active_deployment_id: project.active_deployment_id,
            active_deployment,
            rollback_deployment_id,
        })
    }

    pub async fn get_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<ReleaseState, ReleaseError> {
        let project = self
            .load_visible_project(database, actor, project_id)
            .await?;
        self.build_state(database, &project).await
    }

    pub async fn activate_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<ReleaseState, ReleaseError> {
        let project = releaseable_project(
            self.load_visible_project(database, actor, project_id)
                .await?,
        )?;
        let deployment = self
            .load_project_deployment(database, project.id, deployment_id)
            .await?;
        if deployment.status != deployment::DeploymentStatus::Ready {
            return Err(ReleaseError::conflict("deployment is not ready"));
        }

        let artifacts = deployment_artifact_repository(database)
            .list_by_deployment(deployment.id)
            .await
            .map_err(map_db_error)?;
        if static_site_artifact(&artifacts).is_none() {
            return Err(ReleaseError::conflict(
                "deployment does not have a static_site artifact",
            ));
        }

        let updated_project = project_repository(database)
            .set_active_deployment(project.id, Some(deployment.id), Utc::now())
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ReleaseError::not_found("project not found"))?;

        self.build_state(database, &updated_project).await
    }

    pub async fn rollback_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<ReleaseState, ReleaseError> {
        let project = releaseable_project(
            self.load_visible_project(database, actor, project_id)
                .await?,
        )?;
        let active_deployment_id = project
            .active_deployment_id
            .ok_or_else(|| ReleaseError::conflict("project has no active release"))?;
        let rollback_deployment = self
            .rollback_candidate_for_project(database, project.id, active_deployment_id)
            .await?
            .ok_or_else(|| ReleaseError::conflict("no previous ready release available"))?;

        let updated_project = project_repository(database)
            .set_active_deployment(project.id, Some(rollback_deployment.id), Utc::now())
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ReleaseError::not_found("project not found"))?;

        self.build_state(database, &updated_project).await
    }

    pub async fn resolve_active_site(
        &self,
        database: &DatabaseConnection,
        project_slug: &str,
    ) -> Result<Option<ActiveSiteRelease>, ReleaseError> {
        let project = match project_repository(database)
            .find_by_slug(project_slug)
            .await
            .map_err(map_db_error)?
        {
            Some(project) if project.status != project_entity::ProjectStatus::SoftDeleted => {
                project
            }
            Some(_) | None => return Ok(None),
        };
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

        Ok(Some(ActiveSiteRelease {
            project_slug: project.slug,
            root_dir: artifact.storage_path,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use grass_worker_database::entities::{project, project_host_binding};
    use sea_orm::{DatabaseBackend, MockDatabase};

    fn actor(id: Uuid) -> AuthenticatedUser {
        AuthenticatedUser {
            id,
            email: "owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
        }
    }

    fn sample_project(id: Uuid, owner_user_id: Uuid) -> project::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 11, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id,
            active_deployment_id: None,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: project::ProjectStatus::Active,
            created_at: now,
            updated_at: now,
            archived_at: None,
            soft_deleted_at: None,
        }
    }

    #[tokio::test]
    async fn get_for_project_returns_primary_host_when_present() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 11, 5, 0).unwrap();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(project_id, owner_id)]])
            .append_query_results([[project_host_binding::Model {
                id: Uuid::new_v4(),
                project_id,
                source_id: None,
                host: "docs.example.com".to_owned(),
                is_primary: true,
                created_at: now,
                updated_at: now,
            }]])
            .into_connection();

        let release = ReleaseService
            .get_for_project(&database, &actor(owner_id), project_id)
            .await
            .unwrap();

        assert_eq!(release.primary_host.as_deref(), Some("docs.example.com"));
    }
}
