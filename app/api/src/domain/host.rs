use crate::domain::{
    auth::AuthenticatedUser,
    project::{self, enforce_project_visibility},
};
use chrono::Utc;
use grass_worker_database::entities::{
    platform_host_source, project as project_entity, project_host_binding,
};
use grass_worker_database::repository::{
    NewProjectHostBinding, PlatformHostSourceRepository, ProjectHostBindingRepository,
    ProjectRepository, SeaOrmPlatformHostSourceRepository, SeaOrmProjectHostBindingRepository,
    SeaOrmProjectRepository,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectHostInput {
    pub source_id: Option<Uuid>,
    pub host: String,
    pub is_primary: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostErrorKind {
    Validation,
    NotFound,
    Forbidden,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    kind: HostErrorKind,
    message: String,
}

impl HostError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: HostErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: HostErrorKind::NotFound,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: HostErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: HostErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: HostErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &HostErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

fn map_project_error(error: project::ProjectError) -> HostError {
    match error.kind() {
        project::ProjectErrorKind::Validation => HostError::validation(error.message()),
        project::ProjectErrorKind::NotFound => HostError::not_found(error.message()),
        project::ProjectErrorKind::Forbidden => HostError::forbidden(error.message()),
        project::ProjectErrorKind::Conflict => HostError::conflict(error.message()),
        project::ProjectErrorKind::Internal => HostError::internal(error.message()),
    }
}

fn map_db_error(error: sea_orm::DbErr) -> HostError {
    tracing::error!(error = %error, "host database operation failed");
    HostError::internal(error.to_string())
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

fn host_source_repository(database: &DatabaseConnection) -> SeaOrmPlatformHostSourceRepository {
    SeaOrmPlatformHostSourceRepository::new(clone_database_connection(database))
}

fn host_binding_repository(database: &DatabaseConnection) -> SeaOrmProjectHostBindingRepository {
    SeaOrmProjectHostBindingRepository::new(clone_database_connection(database))
}

fn host_bindable_project(
    project: project_entity::Model,
) -> Result<project_entity::Model, HostError> {
    if project.status == project_entity::ProjectStatus::SoftDeleted {
        return Err(HostError::conflict("project is soft deleted"));
    }

    Ok(project)
}

fn host_matches_source_base_domain(
    host: &str,
    source: &platform_host_source::Model,
) -> Result<bool, HostError> {
    let base_domain = normalize_host(&source.base_domain)?;
    Ok(host == base_domain || host.ends_with(format!(".{base_domain}").as_str()))
}

pub fn normalize_host(host: &str) -> Result<String, HostError> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err(HostError::validation("host is required"));
    }

    let without_port = match trimmed.rsplit_once(':') {
        Some((value, port))
            if !value.is_empty()
                && !value.contains(']')
                && !value.contains('/')
                && !value.contains(':')
                && !port.is_empty()
                && port.bytes().all(|ch| ch.is_ascii_digit()) =>
        {
            value
        }
        _ => trimmed,
    };
    let normalized = without_port.trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(HostError::validation("host is required"));
    }

    Ok(normalized)
}

impl HostService {
    async fn load_visible_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
    ) -> Result<project_entity::Model, HostError> {
        let project = project_repository(database)
            .find_by_id(project_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| HostError::not_found("project not found"))?;

        enforce_project_visibility(actor, project).map_err(map_project_error)
    }

    async fn load_source(
        &self,
        database: &DatabaseConnection,
        source_id: Uuid,
    ) -> Result<platform_host_source::Model, HostError> {
        let source = host_source_repository(database)
            .find_by_id(source_id)
            .await
            .map_err(map_db_error)?
            .ok_or_else(|| HostError::not_found("host source not found"))?;
        if !source.enabled {
            return Err(HostError::conflict("host source is disabled"));
        }

        Ok(source)
    }

    pub async fn create_for_project(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
        project_id: Uuid,
        input: CreateProjectHostInput,
    ) -> Result<project_host_binding::Model, HostError> {
        let project = host_bindable_project(
            self.load_visible_project(database, actor, project_id)
                .await?,
        )?;
        let host = normalize_host(&input.host)?;
        let source_id = match input.source_id {
            Some(source_id) => {
                let source = self.load_source(database, source_id).await?;
                if !host_matches_source_base_domain(&host, &source)? {
                    return Err(HostError::validation("host must match source base domain"));
                }
                Some(source_id)
            }
            None => None,
        };
        let existing_bindings = host_binding_repository(database)
            .list_by_project(project.id)
            .await
            .map_err(map_db_error)?;
        let should_promote = input
            .is_primary
            .unwrap_or_else(|| existing_bindings.iter().all(|binding| !binding.is_primary));
        let create_as_primary =
            should_promote && existing_bindings.iter().all(|binding| !binding.is_primary);

        let created = host_binding_repository(database)
            .create(NewProjectHostBinding {
                id: Uuid::new_v4(),
                project_id: project.id,
                source_id,
                host,
                is_primary: create_as_primary,
                created_at: Utc::now(),
            })
            .await
            .map_err(map_db_error)?;

        if should_promote && !created.is_primary {
            return host_binding_repository(database)
                .set_primary(created.id, Utc::now())
                .await
                .map_err(map_db_error)?
                .ok_or_else(|| HostError::internal("failed to promote host binding to primary"));
        }

        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use grass_worker_database::entities::project;
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};

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
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 9, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id,
            active_deployment_id: None,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status,
            created_at: now,
            updated_at: now,
            archived_at: None,
            soft_deleted_at: None,
        }
    }

    #[test]
    fn normalize_host_strips_port_and_trailing_dot() {
        assert_eq!(
            normalize_host("  Docs.Example.com.:443  ").unwrap(),
            "docs.example.com"
        );
    }

    #[tokio::test]
    async fn create_binding_promotes_first_binding_to_primary() {
        let owner_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[sample_project(
                project_id,
                owner_id,
                project::ProjectStatus::Active,
            )]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();

        let binding = HostService
            .create_for_project(
                &database,
                &actor(owner_id, false),
                project_id,
                CreateProjectHostInput {
                    source_id: None,
                    host: " Docs.Example.com ".to_owned(),
                    is_primary: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(binding.host, "docs.example.com");
        assert!(binding.is_primary);
    }
}
