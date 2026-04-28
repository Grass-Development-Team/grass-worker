use crate::domain::auth::AuthenticatedUser;
use chrono::Utc;
use grass_worker_database::entities::project;
use grass_worker_database::repository::{
    NewProject, ProjectListFilter, ProjectRepository, SeaOrmProjectRepository,
    SeaOrmUserRepository, UpdateProject, UserRepository,
};
use sea_orm::{DatabaseConnection, DbErr};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectErrorKind {
    Validation,
    NotFound,
    Forbidden,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectError {
    kind: ProjectErrorKind,
    message: String,
}

impl ProjectError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: ProjectErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: ProjectErrorKind::NotFound,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: ProjectErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: ProjectErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: ProjectErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &ProjectErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectListStatus {
    Active,
    Archived,
    SoftDeleted,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreProjectStatus {
    Active,
    Archived,
}

pub fn map_db_error(error: DbErr) -> ProjectError {
    tracing::error!(error = %error, "project database operation failed");
    ProjectError::internal(error.to_string())
}

pub fn normalize_name(name: &str) -> Result<String, ProjectError> {
    let normalized = name.trim();
    if normalized.is_empty() {
        return Err(ProjectError::validation("name is required"));
    }
    if normalized.chars().count() > 120 {
        return Err(ProjectError::validation(
            "name must be at most 120 characters",
        ));
    }

    Ok(normalized.to_owned())
}

pub fn normalize_slug(slug: &str) -> Result<String, ProjectError> {
    let normalized = slug.trim();
    if normalized.is_empty() {
        return Err(ProjectError::validation("slug is required"));
    }
    if normalized.chars().count() > 63 {
        return Err(ProjectError::validation(
            "slug must be at most 63 characters",
        ));
    }
    if normalized.starts_with('-') || normalized.ends_with('-') {
        return Err(ProjectError::validation(
            "slug cannot start or end with hyphen",
        ));
    }
    if !normalized
        .bytes()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-')
    {
        return Err(ProjectError::validation(
            "slug can only contain lowercase letters, numbers, and hyphens",
        ));
    }

    Ok(normalized.to_owned())
}

pub fn enforce_project_visibility(
    actor: &AuthenticatedUser,
    project: project::Model,
) -> Result<project::Model, ProjectError> {
    if actor.is_admin {
        return Ok(project);
    }

    if project.owner_user_id == actor.id && project.status != project::ProjectStatus::SoftDeleted {
        return Ok(project);
    }

    Err(ProjectError::not_found("project not found"))
}

pub fn restore_status_to_project_status(status: RestoreProjectStatus) -> project::ProjectStatus {
    match status {
        RestoreProjectStatus::Active => project::ProjectStatus::Active,
        RestoreProjectStatus::Archived => project::ProjectStatus::Archived,
    }
}

fn map_list_filter(
    actor: &AuthenticatedUser,
    status: ProjectListStatus,
) -> Result<ProjectListFilter, ProjectError> {
    match status {
        ProjectListStatus::Active => Ok(ProjectListFilter::Active),
        ProjectListStatus::Archived => Ok(ProjectListFilter::Archived),
        ProjectListStatus::SoftDeleted if actor.is_admin => Ok(ProjectListFilter::SoftDeleted),
        ProjectListStatus::SoftDeleted => Err(ProjectError::forbidden("forbidden")),
        ProjectListStatus::All if actor.is_admin => Ok(ProjectListFilter::All),
        ProjectListStatus::All => Ok(ProjectListFilter::ActiveAndArchived),
    }
}

fn is_slug_conflict(error: &DbErr) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    (message.contains("duplicate") || message.contains("unique")) && message.contains("slug")
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

fn user_repository(database: &DatabaseConnection) -> SeaOrmUserRepository {
    SeaOrmUserRepository::new(clone_database_connection(database))
}

fn ensure_owner_or_admin(
    actor: &AuthenticatedUser,
    project: &project::Model,
) -> Result<(), ProjectError> {
    if actor.is_admin || project.owner_user_id == actor.id {
        return Ok(());
    }

    Err(ProjectError::not_found("project not found"))
}

fn ensure_can_archive(
    actor: &AuthenticatedUser,
    project: &project::Model,
) -> Result<(), ProjectError> {
    ensure_owner_or_admin(actor, project)?;
    if project.status != project::ProjectStatus::Active {
        return Err(ProjectError::conflict("project is not active"));
    }

    Ok(())
}

fn ensure_can_unarchive(
    actor: &AuthenticatedUser,
    project: &project::Model,
) -> Result<(), ProjectError> {
    ensure_owner_or_admin(actor, project)?;
    if project.status != project::ProjectStatus::Archived {
        return Err(ProjectError::conflict("project is not archived"));
    }

    Ok(())
}

fn ensure_can_soft_delete(
    actor: &AuthenticatedUser,
    project: &project::Model,
) -> Result<(), ProjectError> {
    ensure_owner_or_admin(actor, project)?;
    if project.status == project::ProjectStatus::SoftDeleted {
        return Err(ProjectError::conflict("project is already soft deleted"));
    }

    Ok(())
}

fn ensure_can_restore(
    actor: &AuthenticatedUser,
    project: &project::Model,
    _status: RestoreProjectStatus,
) -> Result<(), ProjectError> {
    if !actor.is_admin {
        return Err(ProjectError::forbidden("forbidden"));
    }
    if project.status != project::ProjectStatus::SoftDeleted {
        return Err(ProjectError::conflict("project is not soft deleted"));
    }

    Ok(())
}

fn ensure_can_transfer_owner(
    actor: &AuthenticatedUser,
    project: &project::Model,
) -> Result<(), ProjectError> {
    ensure_owner_or_admin(actor, project)?;
    if project.status == project::ProjectStatus::SoftDeleted {
        return Err(ProjectError::conflict(
            "soft deleted projects must be restored before transfer",
        ));
    }

    Ok(())
}

fn stale_transition_conflict() -> ProjectError {
    ProjectError::conflict("project state changed; retry")
}

impl ProjectService {
    async fn list_with_filter(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        filter: ProjectListFilter,
    ) -> Result<Vec<project::Model>, ProjectError> {
        let repository = project_repository(database);

        if actor.is_admin {
            repository.list_all(filter).await.map_err(map_db_error)
        } else {
            repository
                .list_by_owner(actor.id, filter)
                .await
                .map_err(map_db_error)
        }
    }

    pub async fn create(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        name: &str,
        slug: &str,
    ) -> Result<project::Model, ProjectError> {
        let name = normalize_name(name)?;
        let slug = normalize_slug(slug)?;
        let repository = project_repository(database);

        if repository
            .find_by_slug(&slug)
            .await
            .map_err(map_db_error)?
            .is_some()
        {
            return Err(ProjectError::conflict("slug already exists"));
        }

        let created_at = Utc::now();
        repository
            .create(NewProject {
                id: Uuid::new_v4(),
                owner_user_id: actor.id,
                slug,
                name,
                created_at,
            })
            .await
            .map_err(|error| {
                if is_slug_conflict(&error) {
                    ProjectError::conflict("slug already exists")
                } else {
                    map_db_error(error)
                }
            })
    }

    pub async fn list(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        status: ProjectListStatus,
    ) -> Result<Vec<project::Model>, ProjectError> {
        let filter = map_list_filter(actor, status)?;
        self.list_with_filter(database, actor, filter).await
    }

    pub async fn list_default(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
    ) -> Result<Vec<project::Model>, ProjectError> {
        self.list_with_filter(database, actor, ProjectListFilter::ActiveAndArchived)
            .await
    }

    pub async fn get(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
    ) -> Result<project::Model, ProjectError> {
        let repository = project_repository(database);
        let project = repository
            .find_by_id(id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ProjectError::not_found("project not found"))?;

        enforce_project_visibility(actor, project)
    }

    pub async fn archive(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
    ) -> Result<project::Model, ProjectError> {
        let project = self.get(database, actor, id).await?;
        ensure_can_archive(actor, &project)?;
        let now = Utc::now();

        match project_repository(database)
            .set_status_if_current(
                id,
                project.status,
                project::ProjectStatus::Archived,
                now,
                Some(now),
                None,
            )
            .await
            .map_err(map_db_error)?
        {
            Some(project) => Ok(project),
            None => {
                let current = self.get(database, actor, id).await?;
                ensure_can_archive(actor, &current)?;
                Err(stale_transition_conflict())
            }
        }
    }

    pub async fn unarchive(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
    ) -> Result<project::Model, ProjectError> {
        let project = self.get(database, actor, id).await?;
        ensure_can_unarchive(actor, &project)?;

        match project_repository(database)
            .set_status_if_current(
                id,
                project.status,
                project::ProjectStatus::Active,
                Utc::now(),
                None,
                None,
            )
            .await
            .map_err(map_db_error)?
        {
            Some(project) => Ok(project),
            None => {
                let current = self.get(database, actor, id).await?;
                ensure_can_unarchive(actor, &current)?;
                Err(stale_transition_conflict())
            }
        }
    }

    pub async fn soft_delete(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
    ) -> Result<project::Model, ProjectError> {
        let project = self.get(database, actor, id).await?;
        ensure_can_soft_delete(actor, &project)?;
        let now = Utc::now();

        match project_repository(database)
            .set_status_if_current(
                id,
                project.status,
                project::ProjectStatus::SoftDeleted,
                now,
                None,
                Some(now),
            )
            .await
            .map_err(map_db_error)?
        {
            Some(project) => Ok(project),
            None => {
                let current = self.get(database, actor, id).await?;
                ensure_can_soft_delete(actor, &current)?;
                Err(stale_transition_conflict())
            }
        }
    }

    pub async fn restore(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
        status: RestoreProjectStatus,
    ) -> Result<project::Model, ProjectError> {
        let repository = project_repository(database);
        let project = repository
            .find_by_id(id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ProjectError::not_found("project not found"))?;
        ensure_can_restore(actor, &project, status)?;
        let now = Utc::now();
        let next_status = restore_status_to_project_status(status);
        let archived_at = if next_status == project::ProjectStatus::Archived {
            Some(now)
        } else {
            None
        };

        match repository
            .set_status_if_current(id, project.status, next_status, now, archived_at, None)
            .await
            .map_err(map_db_error)?
        {
            Some(project) => Ok(project),
            None => {
                let current = repository
                    .find_by_id(id)
                    .await
                    .map_err(map_db_error)?
                    .ok_or_else(|| ProjectError::not_found("project not found"))?;
                ensure_can_restore(actor, &current, status)?;
                Err(stale_transition_conflict())
            }
        }
    }

    pub async fn transfer_owner(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
        owner_email: &str,
    ) -> Result<project::Model, ProjectError> {
        let project = self.get(database, actor, id).await?;
        ensure_can_transfer_owner(actor, &project)?;
        let owner_email = owner_email.trim().to_ascii_lowercase();
        if owner_email.is_empty() {
            return Err(ProjectError::validation("owner_email is required"));
        }
        let target = user_repository(database)
            .find_by_email(&owner_email)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ProjectError::not_found("user not found"))?;
        if target.id == project.owner_user_id {
            return Err(ProjectError::conflict(
                "project already belongs to that user",
            ));
        }

        match project_repository(database)
            .transfer_owner_if_current(
                id,
                project.owner_user_id,
                project.status,
                target.id,
                Utc::now(),
            )
            .await
            .map_err(map_db_error)?
        {
            Some(project) => Ok(project),
            None => {
                let current = self.get(database, actor, id).await?;
                ensure_can_transfer_owner(actor, &current)?;
                if current.owner_user_id == target.id {
                    return Err(ProjectError::conflict(
                        "project already belongs to that user",
                    ));
                }
                Err(stale_transition_conflict())
            }
        }
    }

    pub async fn hard_delete(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
    ) -> Result<(), ProjectError> {
        if !actor.is_admin {
            return Err(ProjectError::forbidden("forbidden"));
        }

        let repository = project_repository(database);
        let project = repository
            .find_by_id(id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| ProjectError::not_found("project not found"))?;
        if project.status != project::ProjectStatus::SoftDeleted {
            return Err(ProjectError::conflict(
                "project must be soft deleted before hard delete",
            ));
        }
        if repository
            .has_deployments(project.id)
            .await
            .map_err(map_db_error)?
        {
            return Err(ProjectError::conflict("project still has deployments"));
        }
        if repository
            .hard_delete(project.id)
            .await
            .map_err(map_db_error)?
        {
            return Ok(());
        }

        let current = repository.find_by_id(id).await.map_err(map_db_error)?;
        let Some(current) = current else {
            return Err(ProjectError::not_found("project not found"));
        };
        if current.status != project::ProjectStatus::SoftDeleted {
            return Err(ProjectError::conflict(
                "project must be soft deleted before hard delete",
            ));
        }
        if repository
            .has_deployments(current.id)
            .await
            .map_err(map_db_error)?
        {
            return Err(ProjectError::conflict("project still has deployments"));
        }

        Err(stale_transition_conflict())
    }

    pub async fn update(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        id: Uuid,
        name: Option<&str>,
        slug: Option<&str>,
    ) -> Result<project::Model, ProjectError> {
        let existing = self.get(database, actor, id).await?;
        if existing.status == project::ProjectStatus::SoftDeleted {
            return Err(ProjectError::conflict(
                "soft deleted projects must be restored before editing",
            ));
        }
        if name.is_none() && slug.is_none() {
            return Ok(existing);
        }

        let name = match name {
            Some(value) => normalize_name(value)?,
            None => existing.name.clone(),
        };
        let slug = match slug {
            Some(value) => normalize_slug(value)?,
            None => existing.slug.clone(),
        };
        let repository = project_repository(database);

        if slug != existing.slug
            && let Some(found) = repository.find_by_slug(&slug).await.map_err(map_db_error)?
            && found.id != existing.id
        {
            return Err(ProjectError::conflict("slug already exists"));
        }

        repository
            .update_details(
                id,
                UpdateProject {
                    name,
                    slug,
                    updated_at: Utc::now(),
                },
            )
            .await
            .map_err(|error| {
                if is_slug_conflict(&error) {
                    ProjectError::conflict("slug already exists")
                } else {
                    map_db_error(error)
                }
            })?
            .ok_or_else(|| {
                ProjectError::conflict("soft deleted projects must be restored before editing")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sea_orm::{DatabaseBackend, DbErr, MockDatabase, MockExecResult};

    fn sample_actor(is_admin: bool) -> AuthenticatedUser {
        AuthenticatedUser {
            id: Uuid::new_v4(),
            email: "owner@example.com".to_owned(),
            is_admin,
            is_initial_admin: is_admin,
        }
    }

    fn sample_project(owner_user_id: Uuid, status: project::ProjectStatus) -> project::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 23, 8, 0, 0).unwrap();
        let archived_at = if status == project::ProjectStatus::Archived {
            Some(Utc.with_ymd_and_hms(2026, 4, 23, 8, 30, 0).unwrap())
        } else {
            None
        };
        let soft_deleted_at = if status == project::ProjectStatus::SoftDeleted {
            Some(Utc.with_ymd_and_hms(2026, 4, 23, 9, 0, 0).unwrap())
        } else {
            None
        };

        project::Model {
            id: Uuid::new_v4(),
            owner_user_id,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status,
            created_at,
            updated_at: created_at,
            archived_at,
            soft_deleted_at,
        }
    }

    fn sample_project_with(
        id: Uuid,
        owner_user_id: Uuid,
        status: project::ProjectStatus,
        slug: &str,
        name: &str,
    ) -> project::Model {
        let created_at = Utc.with_ymd_and_hms(2026, 4, 23, 8, 0, 0).unwrap();
        let archived_at = if status == project::ProjectStatus::Archived {
            Some(Utc.with_ymd_and_hms(2026, 4, 23, 8, 30, 0).unwrap())
        } else {
            None
        };
        let soft_deleted_at = if status == project::ProjectStatus::SoftDeleted {
            Some(Utc.with_ymd_and_hms(2026, 4, 23, 9, 0, 0).unwrap())
        } else {
            None
        };

        project::Model {
            id,
            owner_user_id,
            slug: slug.to_owned(),
            name: name.to_owned(),
            status,
            created_at,
            updated_at: created_at,
            archived_at,
            soft_deleted_at,
        }
    }

    #[test]
    fn normalize_slug_accepts_lowercase_dash_format() {
        assert_eq!(normalize_slug(" docs-site-v2 ").unwrap(), "docs-site-v2");
    }

    #[test]
    fn normalize_slug_rejects_uppercase_letters() {
        let error = normalize_slug("Docs-Site").unwrap_err();
        assert_eq!(error.kind(), &ProjectErrorKind::Validation);
    }

    #[test]
    fn owner_can_manage_owned_project_but_not_soft_deleted_visibility() {
        let owner = sample_actor(false);
        let active = sample_project(owner.id, project::ProjectStatus::Active);
        assert!(enforce_project_visibility(&owner, active).is_ok());

        let soft_deleted = sample_project(owner.id, project::ProjectStatus::SoftDeleted);
        let error = enforce_project_visibility(&owner, soft_deleted).unwrap_err();
        assert_eq!(error.kind(), &ProjectErrorKind::NotFound);
    }

    #[test]
    fn admin_can_restore_soft_deleted_project_to_archived() {
        let admin = sample_actor(true);
        let soft_deleted = sample_project(Uuid::new_v4(), project::ProjectStatus::SoftDeleted);

        let visible = enforce_project_visibility(&admin, soft_deleted).unwrap();
        assert_eq!(visible.status, project::ProjectStatus::SoftDeleted);
        assert_eq!(
            restore_status_to_project_status(RestoreProjectStatus::Archived),
            project::ProjectStatus::Archived
        );
    }

    #[tokio::test]
    async fn create_returns_conflict_when_slug_already_exists() {
        let actor = sample_actor(false);
        let existing = sample_project_with(
            Uuid::new_v4(),
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .create(&database, &actor, "Docs Site", "docs-site")
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(error.message(), "slug already exists");
    }

    #[tokio::test]
    async fn list_returns_forbidden_for_non_admin_soft_deleted_status() {
        let actor = sample_actor(false);
        let database = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
        let service = ProjectService;

        let error = service
            .list(&database, &actor, ProjectListStatus::SoftDeleted)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Forbidden);
        assert_eq!(error.message(), "forbidden");
    }

    #[tokio::test]
    async fn get_returns_not_found_for_non_admin_soft_deleted_owned_project() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let soft_deleted = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[soft_deleted]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .get(&database, &actor, project_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::NotFound);
        assert_eq!(error.message(), "project not found");
    }

    #[tokio::test]
    async fn update_maps_write_time_duplicate_slug_failure_to_conflict() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results(vec![vec![existing.clone()], vec![], vec![existing.clone()]])
            .append_exec_errors([DbErr::Custom("duplicate key: slug".to_owned())])
            .into_connection();
        let service = ProjectService;

        let error = service
            .update(
                &database,
                &actor,
                project_id,
                Some("Docs Site V2"),
                Some("docs-v2"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(error.message(), "slug already exists");
    }

    #[tokio::test]
    async fn update_with_no_fields_returns_existing_without_calling_update_details() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [existing.clone()], [existing.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let updated = service
            .update(&database, &actor, project_id, None, None)
            .await
            .unwrap();

        assert_eq!(updated, existing);

        let statements = database.into_transaction_log();
        assert!(
            !statements
                .iter()
                .flat_map(|entry| entry.statements().iter())
                .any(|statement| statement.sql.contains("UPDATE \"projects\""))
        );
    }

    #[tokio::test]
    async fn update_rejects_stale_soft_deleted_row_without_mutation() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [existing.clone()], [existing.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 0,
            }])
            .into_connection();
        let service = ProjectService;

        let error = service
            .update(&database, &actor, project_id, Some("Docs Site V2"), None)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(
            error.message(),
            "soft deleted projects must be restored before editing"
        );
    }

    #[tokio::test]
    async fn owner_can_archive_active_project() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let archived_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            status: project::ProjectStatus::Archived,
            updated_at: archived_at,
            archived_at: Some(archived_at),
            soft_deleted_at: None,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let archived = service
            .archive(&database, &actor, project_id)
            .await
            .unwrap();

        assert_eq!(archived, expected);
    }

    #[tokio::test]
    async fn owner_can_unarchive_archived_project() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let restored_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            status: project::ProjectStatus::Active,
            updated_at: restored_at,
            archived_at: None,
            soft_deleted_at: None,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let unarchived = service
            .unarchive(&database, &actor, project_id)
            .await
            .unwrap();

        assert_eq!(unarchived, expected);
    }

    #[tokio::test]
    async fn owner_can_soft_delete_archived_project() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let soft_deleted_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            status: project::ProjectStatus::SoftDeleted,
            updated_at: soft_deleted_at,
            archived_at: None,
            soft_deleted_at: Some(soft_deleted_at),
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let deleted = service
            .soft_delete(&database, &actor, project_id)
            .await
            .unwrap();

        assert_eq!(deleted, expected);
    }

    #[tokio::test]
    async fn restore_rejects_non_soft_deleted_projects() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .restore(&database, &actor, project_id, RestoreProjectStatus::Active)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(error.message(), "project is not soft deleted");
    }

    #[tokio::test]
    async fn restore_to_archived_sets_archived_at() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let restored_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            status: project::ProjectStatus::Archived,
            updated_at: restored_at,
            archived_at: Some(restored_at),
            soft_deleted_at: None,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let restored = service
            .restore(
                &database,
                &actor,
                project_id,
                RestoreProjectStatus::Archived,
            )
            .await
            .unwrap();

        assert_eq!(restored, expected);
    }

    #[tokio::test]
    async fn restore_to_active_clears_archived_and_soft_deleted_timestamps() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let restored_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            status: project::ProjectStatus::Active,
            updated_at: restored_at,
            archived_at: None,
            soft_deleted_at: None,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()], [expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let restored = service
            .restore(&database, &actor, project_id, RestoreProjectStatus::Active)
            .await
            .unwrap();

        assert_eq!(restored, expected);
    }

    #[tokio::test]
    async fn restore_requires_admin() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .restore(&database, &actor, project_id, RestoreProjectStatus::Active)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Forbidden);
        assert_eq!(error.message(), "forbidden");
    }

    #[tokio::test]
    async fn archive_rejects_stale_state_changes() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let mut soft_deleted = existing.clone();
        soft_deleted.status = project::ProjectStatus::SoftDeleted;
        soft_deleted.updated_at = existing.updated_at + chrono::Duration::hours(1);
        soft_deleted.archived_at = None;
        soft_deleted.soft_deleted_at = Some(soft_deleted.updated_at);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing], [soft_deleted]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 0,
            }])
            .into_connection();
        let service = ProjectService;

        let error = service
            .archive(&database, &actor, project_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(error.message(), "project is not active");
    }

    #[tokio::test]
    async fn transfer_owner_requires_existing_target_user() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .append_query_results([Vec::<grass_worker_database::entities::user::Model>::new()])
            .into_connection();
        let service = ProjectService;

        let error = service
            .transfer_owner(&database, &actor, project_id, "missing@example.com")
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::NotFound);
        assert_eq!(error.message(), "user not found");
    }

    #[tokio::test]
    async fn transfer_owner_updates_project_owner_for_admin() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let target_user = grass_worker_database::entities::user::Model {
            id: Uuid::new_v4(),
            email: "new-owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: existing.created_at,
            updated_at: existing.created_at,
        };
        let updated_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            owner_user_id: target_user.id,
            updated_at,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()]])
            .append_query_results([[target_user]])
            .append_query_results([[expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let transferred = service
            .transfer_owner(&database, &actor, project_id, "new-owner@example.com")
            .await
            .unwrap();

        assert_eq!(transferred, expected);
    }

    #[tokio::test]
    async fn transfer_owner_rejects_blank_owner_email() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .transfer_owner(&database, &actor, project_id, "   ")
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Validation);
        assert_eq!(error.message(), "owner_email is required");
    }

    #[tokio::test]
    async fn owner_can_transfer_project_to_existing_user() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let target_user = grass_worker_database::entities::user::Model {
            id: Uuid::new_v4(),
            email: "new-owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: existing.created_at,
            updated_at: existing.created_at,
        };
        let updated_at = existing.updated_at + chrono::Duration::hours(1);
        let expected = project::Model {
            owner_user_id: target_user.id,
            updated_at,
            ..existing.clone()
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing.clone()]])
            .append_query_results([[target_user]])
            .append_query_results([[expected.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        let transferred = service
            .transfer_owner(&database, &actor, project_id, "new-owner@example.com")
            .await
            .unwrap();

        assert_eq!(transferred, expected);
    }

    #[tokio::test]
    async fn transfer_owner_rejects_soft_deleted_project() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .transfer_owner(&database, &actor, project_id, "new-owner@example.com")
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(
            error.message(),
            "soft deleted projects must be restored before transfer"
        );
    }

    #[tokio::test]
    async fn hard_delete_requires_soft_deleted_status() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Archived,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .hard_delete(&database, &actor, project_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(
            error.message(),
            "project must be soft deleted before hard delete"
        );
    }

    #[tokio::test]
    async fn hard_delete_succeeds_for_admin_soft_deleted_project() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .append_query_results([
                Vec::<grass_worker_database::entities::deployment::Model>::new(),
            ])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let service = ProjectService;

        service
            .hard_delete(&database, &actor, project_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn hard_delete_rejects_projects_with_deployments() {
        let actor = sample_actor(true);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            Uuid::new_v4(),
            project::ProjectStatus::SoftDeleted,
            "docs-site",
            "Docs Site",
        );
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .append_query_results([[grass_worker_database::entities::deployment::Model {
                id: Uuid::new_v4(),
                project_id,
                status: grass_worker_database::entities::deployment::DeploymentStatus::Pending,
                source_branch: None,
                source_revision: None,
                created_at: chrono::Utc::now(),
                started_at: None,
                finished_at: None,
            }]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .hard_delete(&database, &actor, project_id)
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::Conflict);
        assert_eq!(error.message(), "project still has deployments");
    }

    #[tokio::test]
    async fn transfer_owner_rejects_stale_owner_changes() {
        let actor = sample_actor(false);
        let project_id = Uuid::new_v4();
        let existing = sample_project_with(
            project_id,
            actor.id,
            project::ProjectStatus::Active,
            "docs-site",
            "Docs Site",
        );
        let current = project::Model {
            owner_user_id: Uuid::new_v4(),
            ..existing.clone()
        };
        let target_user = grass_worker_database::entities::user::Model {
            id: Uuid::new_v4(),
            email: "new-owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: existing.created_at,
            updated_at: existing.created_at,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[existing]])
            .append_query_results([[target_user]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 0,
            }])
            .append_query_results([[current]])
            .into_connection();
        let service = ProjectService;

        let error = service
            .transfer_owner(&database, &actor, project_id, "new-owner@example.com")
            .await
            .unwrap_err();

        assert_eq!(error.kind(), &ProjectErrorKind::NotFound);
        assert_eq!(error.message(), "project not found");
    }
}
