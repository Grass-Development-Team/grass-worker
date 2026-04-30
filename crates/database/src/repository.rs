use crate::entities::{
    deployment, deployment_artifact, project, user, user_password_credential, user_session,
};
use async_trait::async_trait;
use sea_orm::entity::prelude::DateTimeUtc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Select, Set,
    sea_query::{OnConflict, Query},
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProject {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: DateTimeUtc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectListFilter {
    Active,
    Archived,
    SoftDeleted,
    ActiveAndArchived,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProject {
    pub name: String,
    pub slug: String,
    pub updated_at: DateTimeUtc,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUser {
    pub id: Uuid,
    pub email: String,
    pub created_at: DateTimeUtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUserPasswordCredential {
    pub user_id: Uuid,
    pub password_hash: String,
    pub password_updated_at: DateTimeUtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUserSession {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub created_at: DateTimeUtc,
    pub expires_at: DateTimeUtc,
}

#[async_trait]
pub trait ProjectRepository {
    async fn create(&self, new_project: NewProject) -> Result<project::Model, DbErr>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<project::Model>, DbErr>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<project::Model>, DbErr>;
    async fn list_by_owner(
        &self,
        owner_user_id: Uuid,
        filter: ProjectListFilter,
    ) -> Result<Vec<project::Model>, DbErr>;
    async fn list_all(&self, filter: ProjectListFilter) -> Result<Vec<project::Model>, DbErr>;
    async fn update_details(
        &self,
        id: Uuid,
        update: UpdateProject,
    ) -> Result<Option<project::Model>, DbErr>;
    async fn set_status(
        &self,
        id: Uuid,
        status: project::ProjectStatus,
        updated_at: DateTimeUtc,
        archived_at: Option<DateTimeUtc>,
        soft_deleted_at: Option<DateTimeUtc>,
    ) -> Result<Option<project::Model>, DbErr>;
    async fn set_status_if_current(
        &self,
        id: Uuid,
        current_status: project::ProjectStatus,
        next_status: project::ProjectStatus,
        updated_at: DateTimeUtc,
        archived_at: Option<DateTimeUtc>,
        soft_deleted_at: Option<DateTimeUtc>,
    ) -> Result<Option<project::Model>, DbErr>;
    async fn transfer_owner_if_current(
        &self,
        id: Uuid,
        current_owner_user_id: Uuid,
        current_status: project::ProjectStatus,
        owner_user_id: Uuid,
        updated_at: DateTimeUtc,
    ) -> Result<Option<project::Model>, DbErr>;
    async fn hard_delete(&self, id: Uuid) -> Result<bool, DbErr>;
    async fn has_deployments(&self, id: Uuid) -> Result<bool, DbErr>;
}

#[async_trait]
pub trait DeploymentRepository {
    async fn create(&self, new_deployment: NewDeployment) -> Result<deployment::Model, DbErr>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<deployment::Model>, DbErr>;
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

#[async_trait]
pub trait UserRepository {
    async fn create(&self, new_user: NewUser) -> Result<user::Model, DbErr>;
    async fn create_admin(
        &self,
        new_user: NewUser,
        is_initial_admin: bool,
    ) -> Result<user::Model, DbErr>;
    async fn find_by_email(&self, email: &str) -> Result<Option<user::Model>, DbErr>;
    async fn has_admin(&self) -> Result<bool, DbErr>;
    async fn list_all(&self) -> Result<Vec<user::Model>, DbErr>;
}

#[async_trait]
pub trait UserPasswordCredentialRepository {
    async fn set_password(
        &self,
        new_credential: NewUserPasswordCredential,
    ) -> Result<user_password_credential::Model, DbErr>;
}

#[async_trait]
pub trait UserSessionRepository {
    async fn create(&self, new_session: NewUserSession) -> Result<user_session::Model, DbErr>;
    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<user_session::Model>, DbErr>;
}

#[derive(Debug)]
pub struct SeaOrmProjectRepository {
    database: DatabaseConnection,
}

impl SeaOrmProjectRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }

    #[cfg(test)]
    pub fn into_connection(self) -> DatabaseConnection {
        self.database
    }
}

fn apply_project_list_filter(
    query: Select<project::Entity>,
    filter: ProjectListFilter,
) -> Select<project::Entity> {
    match filter {
        ProjectListFilter::Active => {
            query.filter(project::Column::Status.eq(project::ProjectStatus::Active))
        }
        ProjectListFilter::Archived => {
            query.filter(project::Column::Status.eq(project::ProjectStatus::Archived))
        }
        ProjectListFilter::SoftDeleted => {
            query.filter(project::Column::Status.eq(project::ProjectStatus::SoftDeleted))
        }
        ProjectListFilter::ActiveAndArchived => query.filter(
            Condition::any()
                .add(project::Column::Status.eq(project::ProjectStatus::Active))
                .add(project::Column::Status.eq(project::ProjectStatus::Archived)),
        ),
        ProjectListFilter::All => query,
    }
}

#[async_trait]
impl ProjectRepository for SeaOrmProjectRepository {
    async fn create(&self, new_project: NewProject) -> Result<project::Model, DbErr> {
        let model = project::Model {
            id: new_project.id,
            owner_user_id: new_project.owner_user_id,
            slug: new_project.slug,
            name: new_project.name,
            status: project::ProjectStatus::Active,
            created_at: new_project.created_at,
            updated_at: new_project.created_at,
            archived_at: None,
            soft_deleted_at: None,
        };

        project::Entity::insert(project::ActiveModel {
            id: Set(model.id),
            owner_user_id: Set(model.owner_user_id),
            slug: Set(model.slug.clone()),
            name: Set(model.name.clone()),
            status: Set(model.status.clone()),
            created_at: Set(model.created_at),
            updated_at: Set(model.updated_at),
            archived_at: Set(model.archived_at),
            soft_deleted_at: Set(model.soft_deleted_at),
        })
        .exec_without_returning(&self.database)
        .await?;

        Ok(model)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<project::Model>, DbErr> {
        project::Entity::find_by_id(id).one(&self.database).await
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<project::Model>, DbErr> {
        project::Entity::find()
            .filter(project::Column::Slug.eq(slug))
            .one(&self.database)
            .await
    }

    async fn list_by_owner(
        &self,
        owner_user_id: Uuid,
        filter: ProjectListFilter,
    ) -> Result<Vec<project::Model>, DbErr> {
        apply_project_list_filter(
            project::Entity::find().filter(project::Column::OwnerUserId.eq(owner_user_id)),
            filter,
        )
        .order_by_desc(project::Column::UpdatedAt)
        .all(&self.database)
        .await
    }

    async fn list_all(&self, filter: ProjectListFilter) -> Result<Vec<project::Model>, DbErr> {
        apply_project_list_filter(project::Entity::find(), filter)
            .order_by_desc(project::Column::UpdatedAt)
            .all(&self.database)
            .await
    }

    async fn update_details(
        &self,
        id: Uuid,
        update: UpdateProject,
    ) -> Result<Option<project::Model>, DbErr> {
        let update_result = project::Entity::update_many()
            .set(project::ActiveModel {
                name: Set(update.name),
                slug: Set(update.slug),
                updated_at: Set(update.updated_at),
                ..Default::default()
            })
            .filter(project::Column::Id.eq(id))
            .filter(project::Column::Status.ne(project::ProjectStatus::SoftDeleted))
            .exec(&self.database)
            .await?;
        if update_result.rows_affected == 0 {
            return Ok(None);
        }

        project::Entity::find_by_id(id).one(&self.database).await
    }

    async fn set_status(
        &self,
        id: Uuid,
        status: project::ProjectStatus,
        updated_at: DateTimeUtc,
        archived_at: Option<DateTimeUtc>,
        soft_deleted_at: Option<DateTimeUtc>,
    ) -> Result<Option<project::Model>, DbErr> {
        let existing = project::Entity::find_by_id(id).one(&self.database).await?;
        let Some(existing) = existing else {
            return Ok(None);
        };

        let mut active_model: project::ActiveModel = existing.into();
        active_model.status = Set(status);
        active_model.updated_at = Set(updated_at);
        active_model.archived_at = Set(archived_at);
        active_model.soft_deleted_at = Set(soft_deleted_at);

        active_model.update(&self.database).await.map(Some)
    }

    async fn set_status_if_current(
        &self,
        id: Uuid,
        current_status: project::ProjectStatus,
        next_status: project::ProjectStatus,
        updated_at: DateTimeUtc,
        archived_at: Option<DateTimeUtc>,
        soft_deleted_at: Option<DateTimeUtc>,
    ) -> Result<Option<project::Model>, DbErr> {
        let update_result = project::Entity::update_many()
            .set(project::ActiveModel {
                status: Set(next_status),
                updated_at: Set(updated_at),
                archived_at: Set(archived_at),
                soft_deleted_at: Set(soft_deleted_at),
                ..Default::default()
            })
            .filter(project::Column::Id.eq(id))
            .filter(project::Column::Status.eq(current_status))
            .exec(&self.database)
            .await?;
        if update_result.rows_affected == 0 {
            return Ok(None);
        }

        project::Entity::find_by_id(id).one(&self.database).await
    }

    async fn transfer_owner_if_current(
        &self,
        id: Uuid,
        current_owner_user_id: Uuid,
        current_status: project::ProjectStatus,
        owner_user_id: Uuid,
        updated_at: DateTimeUtc,
    ) -> Result<Option<project::Model>, DbErr> {
        let update_result = project::Entity::update_many()
            .set(project::ActiveModel {
                owner_user_id: Set(owner_user_id),
                updated_at: Set(updated_at),
                ..Default::default()
            })
            .filter(project::Column::Id.eq(id))
            .filter(project::Column::OwnerUserId.eq(current_owner_user_id))
            .filter(project::Column::Status.eq(current_status))
            .exec(&self.database)
            .await?;
        if update_result.rows_affected == 0 {
            return Ok(None);
        }

        project::Entity::find_by_id(id).one(&self.database).await
    }

    async fn hard_delete(&self, id: Uuid) -> Result<bool, DbErr> {
        let delete_result = project::Entity::delete_many()
            .filter(project::Column::Id.eq(id))
            .filter(project::Column::Status.eq(project::ProjectStatus::SoftDeleted))
            .filter(
                project::Column::Id.not_in_subquery(
                    Query::select()
                        .column(deployment::Column::ProjectId)
                        .from(deployment::Entity)
                        .and_where(deployment::Column::ProjectId.eq(id))
                        .to_owned(),
                ),
            )
            .exec(&self.database)
            .await?;

        Ok(delete_result.rows_affected > 0)
    }

    async fn has_deployments(&self, id: Uuid) -> Result<bool, DbErr> {
        deployment::Entity::find()
            .filter(deployment::Column::ProjectId.eq(id))
            .exists(&self.database)
            .await
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

    #[cfg(test)]
    pub fn into_connection(self) -> DatabaseConnection {
        self.database
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

    async fn find_by_id(&self, id: Uuid) -> Result<Option<deployment::Model>, DbErr> {
        deployment::Entity::find_by_id(id).one(&self.database).await
    }

    async fn list_by_project(&self, project_id: Uuid) -> Result<Vec<deployment::Model>, DbErr> {
        deployment::Entity::find()
            .filter(deployment::Column::ProjectId.eq(project_id))
            .order_by_desc(deployment::Column::CreatedAt)
            .order_by_desc(deployment::Column::Id)
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

#[derive(Debug)]
pub struct SeaOrmUserRepository {
    database: DatabaseConnection,
}

fn build_user_model(new_user: NewUser, is_admin: bool, is_initial_admin: bool) -> user::Model {
    user::Model {
        id: new_user.id,
        email: new_user.email,
        is_admin,
        is_initial_admin,
        created_at: new_user.created_at,
        updated_at: new_user.created_at,
    }
}

pub async fn insert_user<C: ConnectionTrait>(
    connection: &C,
    model: &user::Model,
) -> Result<(), DbErr> {
    user::Entity::insert(user::ActiveModel {
        id: Set(model.id),
        email: Set(model.email.clone()),
        is_admin: Set(model.is_admin),
        is_initial_admin: Set(model.is_initial_admin),
        created_at: Set(model.created_at),
        updated_at: Set(model.updated_at),
    })
    .exec_without_returning(connection)
    .await?;

    Ok(())
}

pub async fn upsert_password_credential<C: ConnectionTrait>(
    connection: &C,
    model: &user_password_credential::Model,
) -> Result<(), DbErr> {
    user_password_credential::Entity::insert(user_password_credential::ActiveModel {
        user_id: Set(model.user_id),
        password_hash: Set(model.password_hash.clone()),
        password_updated_at: Set(model.password_updated_at),
    })
    .on_conflict(
        OnConflict::column(user_password_credential::Column::UserId)
            .update_columns([
                user_password_credential::Column::PasswordHash,
                user_password_credential::Column::PasswordUpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(connection)
    .await?;

    Ok(())
}

pub async fn find_user_by_email<C: ConnectionTrait>(
    connection: &C,
    email: &str,
) -> Result<Option<user::Model>, DbErr> {
    user::Entity::find()
        .filter(user::Column::Email.eq(email))
        .one(connection)
        .await
}

pub async fn find_user_by_id<C: ConnectionTrait>(
    connection: &C,
    id: Uuid,
) -> Result<Option<user::Model>, DbErr> {
    user::Entity::find_by_id(id).one(connection).await
}

pub async fn find_password_credential_by_user_id<C: ConnectionTrait>(
    connection: &C,
    user_id: Uuid,
) -> Result<Option<user_password_credential::Model>, DbErr> {
    user_password_credential::Entity::find_by_id(user_id)
        .one(connection)
        .await
}

pub async fn find_session_by_token_hash<C: ConnectionTrait>(
    connection: &C,
    token_hash: &str,
) -> Result<Option<user_session::Model>, DbErr> {
    user_session::Entity::find()
        .filter(user_session::Column::TokenHash.eq(token_hash))
        .one(connection)
        .await
}

pub async fn insert_session<C: ConnectionTrait>(
    connection: &C,
    model: &user_session::Model,
) -> Result<(), DbErr> {
    user_session::Entity::insert(user_session::ActiveModel {
        id: Set(model.id),
        user_id: Set(model.user_id),
        token_hash: Set(model.token_hash.clone()),
        created_at: Set(model.created_at),
        expires_at: Set(model.expires_at),
        revoked_at: Set(model.revoked_at),
    })
    .exec_without_returning(connection)
    .await?;

    Ok(())
}

pub async fn revoke_session_by_token_hash<C: ConnectionTrait>(
    connection: &C,
    token_hash: &str,
    revoked_at: DateTimeUtc,
) -> Result<(), DbErr> {
    user_session::Entity::update_many()
        .set(user_session::ActiveModel {
            revoked_at: Set(Some(revoked_at)),
            ..Default::default()
        })
        .filter(user_session::Column::TokenHash.eq(token_hash))
        .filter(user_session::Column::RevokedAt.is_null())
        .exec(connection)
        .await?;

    Ok(())
}

impl SeaOrmUserRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl UserRepository for SeaOrmUserRepository {
    async fn create(&self, new_user: NewUser) -> Result<user::Model, DbErr> {
        let model = build_user_model(new_user, false, false);
        insert_user(&self.database, &model).await?;

        Ok(model)
    }

    async fn create_admin(
        &self,
        new_user: NewUser,
        is_initial_admin: bool,
    ) -> Result<user::Model, DbErr> {
        let model = build_user_model(new_user, true, is_initial_admin);
        insert_user(&self.database, &model).await?;

        Ok(model)
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<user::Model>, DbErr> {
        find_user_by_email(&self.database, email).await
    }

    async fn has_admin(&self) -> Result<bool, DbErr> {
        let admin = user::Entity::find()
            .filter(user::Column::IsAdmin.eq(true))
            .one(&self.database)
            .await?;

        Ok(admin.is_some())
    }

    async fn list_all(&self) -> Result<Vec<user::Model>, DbErr> {
        user::Entity::find()
            .order_by_asc(user::Column::CreatedAt)
            .order_by_asc(user::Column::Email)
            .all(&self.database)
            .await
    }
}

#[derive(Debug)]
pub struct SeaOrmUserPasswordCredentialRepository {
    database: DatabaseConnection,
}

impl SeaOrmUserPasswordCredentialRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl UserPasswordCredentialRepository for SeaOrmUserPasswordCredentialRepository {
    async fn set_password(
        &self,
        new_credential: NewUserPasswordCredential,
    ) -> Result<user_password_credential::Model, DbErr> {
        let model = user_password_credential::Model {
            user_id: new_credential.user_id,
            password_hash: new_credential.password_hash,
            password_updated_at: new_credential.password_updated_at,
        };

        upsert_password_credential(&self.database, &model).await?;

        Ok(model)
    }
}

#[derive(Debug)]
pub struct SeaOrmUserSessionRepository {
    database: DatabaseConnection,
}

impl SeaOrmUserSessionRepository {
    pub fn new(database: DatabaseConnection) -> Self {
        Self { database }
    }
}

#[async_trait]
impl UserSessionRepository for SeaOrmUserSessionRepository {
    async fn create(&self, new_session: NewUserSession) -> Result<user_session::Model, DbErr> {
        let model = user_session::Model {
            id: new_session.id,
            user_id: new_session.user_id,
            token_hash: new_session.token_hash,
            created_at: new_session.created_at,
            expires_at: new_session.expires_at,
            revoked_at: None,
        };

        insert_session(&self.database, &model).await?;

        Ok(model)
    }

    async fn find_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<user_session::Model>, DbErr> {
        find_session_by_token_hash(&self.database, token_hash).await
    }
}
