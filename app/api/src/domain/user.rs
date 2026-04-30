use crate::domain::auth::AuthenticatedUser;
use grass_worker_database::entities::user;
use grass_worker_database::repository::{SeaOrmUserRepository, UserRepository};
use sea_orm::{DatabaseConnection, DbErr};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserErrorKind {
    Forbidden,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserError {
    kind: UserErrorKind,
    message: String,
}

impl UserError {
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: UserErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: UserErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &UserErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

fn map_db_error(error: DbErr) -> UserError {
    tracing::error!(error = %error, "user database operation failed");
    UserError::internal(error.to_string())
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

fn user_repository(database: &DatabaseConnection) -> SeaOrmUserRepository {
    SeaOrmUserRepository::new(clone_database_connection(database))
}

impl UserService {
    pub async fn list_all(
        &self,
        database: &DatabaseConnection,
        actor: &AuthenticatedUser,
    ) -> Result<Vec<user::Model>, UserError> {
        if !actor.is_admin {
            return Err(UserError::forbidden("forbidden"));
        }

        user_repository(database)
            .list_all()
            .await
            .map_err(map_db_error)
    }
}
