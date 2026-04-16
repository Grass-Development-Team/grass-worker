use crate::entities::{deployment, deployment_artifact, project};
use async_trait::async_trait;
use sea_orm::entity::prelude::DateTimeUtc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: DateTimeUtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeployment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_branch: Option<String>,
    pub source_revision: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeploymentArtifact {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub kind: deployment_artifact::ArtifactKind,
    pub storage_path: String,
    pub checksum_sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub created_at: DateTimeUtc,
}

#[async_trait]
pub trait ProjectRepository {
    async fn create(&self, new_project: NewProject) -> Result<project::Model, DbErr>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<project::Model>, DbErr>;
    async fn archive(
        &self,
        id: Uuid,
        archived_at: DateTimeUtc,
    ) -> Result<Option<project::Model>, DbErr>;
}

#[async_trait]
pub trait DeploymentRepository {
    async fn create(&self, new_deployment: NewDeployment) -> Result<deployment::Model, DbErr>;
    async fn list_by_project(&self, project_id: Uuid) -> Result<Vec<deployment::Model>, DbErr>;
}

#[async_trait]
pub trait DeploymentArtifactRepository {
    async fn create(
        &self,
        new_artifact: NewDeploymentArtifact,
    ) -> Result<deployment_artifact::Model, DbErr>;
    async fn list_by_deployment(
        &self,
        deployment_id: Uuid,
    ) -> Result<Vec<deployment_artifact::Model>, DbErr>;
}

#[derive(Debug)]
pub struct SeaOrmProjectRepository {
    database: DatabaseConnection,
}

impl SeaOrmProjectRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl ProjectRepository for SeaOrmProjectRepository {
    async fn create(&self, new_project: NewProject) -> Result<project::Model, DbErr> {
        let model = project::Model {
            id: new_project.id,
            slug: new_project.slug,
            name: new_project.name,
            status: project::ProjectStatus::Active,
            created_at: new_project.created_at,
            updated_at: new_project.created_at,
            archived_at: None,
        };

        project::Entity::insert(project::ActiveModel {
            id: Set(model.id),
            slug: Set(model.slug.clone()),
            name: Set(model.name.clone()),
            status: Set(model.status.clone()),
            created_at: Set(model.created_at),
            updated_at: Set(model.updated_at),
            archived_at: Set(model.archived_at),
        })
        .exec_without_returning(&self.database)
        .await?;

        Ok(model)
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<project::Model>, DbErr> {
        project::Entity::find()
            .filter(project::Column::Slug.eq(slug))
            .one(&self.database)
            .await
    }

    async fn archive(
        &self,
        id: Uuid,
        archived_at: DateTimeUtc,
    ) -> Result<Option<project::Model>, DbErr> {
        let existing = project::Entity::find_by_id(id).one(&self.database).await?;
        let Some(existing) = existing else {
            return Ok(None);
        };

        let mut active_model: project::ActiveModel = existing.clone().into();
        active_model.status = Set(project::ProjectStatus::Archived);
        active_model.updated_at = Set(archived_at);
        active_model.archived_at = Set(Some(archived_at));
        active_model.update(&self.database).await?;

        Ok(Some(project::Model {
            status: project::ProjectStatus::Archived,
            updated_at: archived_at,
            archived_at: Some(archived_at),
            ..existing
        }))
    }
}

#[derive(Debug)]
pub struct SeaOrmDeploymentRepository {
    database: DatabaseConnection,
}

impl SeaOrmDeploymentRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl DeploymentRepository for SeaOrmDeploymentRepository {
    async fn create(&self, new_deployment: NewDeployment) -> Result<deployment::Model, DbErr> {
        let model = deployment::Model {
            id: new_deployment.id,
            project_id: new_deployment.project_id,
            status: deployment::DeploymentStatus::Pending,
            source_branch: new_deployment.source_branch,
            source_revision: new_deployment.source_revision,
            created_at: new_deployment.created_at,
            started_at: None,
            finished_at: None,
        };

        deployment::Entity::insert(deployment::ActiveModel {
            id: Set(model.id),
            project_id: Set(model.project_id),
            status: Set(model.status.clone()),
            source_branch: Set(model.source_branch.clone()),
            source_revision: Set(model.source_revision.clone()),
            created_at: Set(model.created_at),
            started_at: Set(model.started_at),
            finished_at: Set(model.finished_at),
        })
        .exec_without_returning(&self.database)
        .await?;

        Ok(model)
    }

    async fn list_by_project(&self, project_id: Uuid) -> Result<Vec<deployment::Model>, DbErr> {
        deployment::Entity::find()
            .filter(deployment::Column::ProjectId.eq(project_id))
            .all(&self.database)
            .await
    }
}

#[derive(Debug)]
pub struct SeaOrmDeploymentArtifactRepository {
    database: DatabaseConnection,
}

impl SeaOrmDeploymentArtifactRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl DeploymentArtifactRepository for SeaOrmDeploymentArtifactRepository {
    async fn create(
        &self,
        new_artifact: NewDeploymentArtifact,
    ) -> Result<deployment_artifact::Model, DbErr> {
        let model = deployment_artifact::Model {
            id: new_artifact.id,
            deployment_id: new_artifact.deployment_id,
            kind: new_artifact.kind,
            storage_path: new_artifact.storage_path,
            checksum_sha256: new_artifact.checksum_sha256,
            size_bytes: new_artifact.size_bytes,
            created_at: new_artifact.created_at,
        };

        deployment_artifact::Entity::insert(deployment_artifact::ActiveModel {
            id: Set(model.id),
            deployment_id: Set(model.deployment_id),
            kind: Set(model.kind.clone()),
            storage_path: Set(model.storage_path.clone()),
            checksum_sha256: Set(model.checksum_sha256.clone()),
            size_bytes: Set(model.size_bytes),
            created_at: Set(model.created_at),
        })
        .exec_without_returning(&self.database)
        .await?;

        Ok(model)
    }

    async fn list_by_deployment(
        &self,
        deployment_id: Uuid,
    ) -> Result<Vec<deployment_artifact::Model>, DbErr> {
        deployment_artifact::Entity::find()
            .filter(deployment_artifact::Column::DeploymentId.eq(deployment_id))
            .all(&self.database)
            .await
    }
}
