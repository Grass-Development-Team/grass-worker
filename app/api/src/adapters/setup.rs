use crate::domain::setup::{
    AdminSetupError, DatabaseInitializer, DatabaseSetupError, InitialAdminCreator,
    InitialAdminInput, SetupBootstrapError, SetupBootstrapper, SharedDatabaseInitializer,
    SharedSetupBootstrapper,
};
use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use async_trait::async_trait;
use chrono::Utc;
use grass_worker_config::DatabaseConfig;
use grass_worker_database::{
    connection::{connect, prepare_schema},
    entities::{user, user_password_credential},
    repository::{SeaOrmUserRepository, UserRepository, insert_user, upsert_password_credential},
};
use grass_worker_migration::{Migrator, MigratorTrait};
use rand_core::OsRng;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter,
    Statement, TransactionTrait,
};
use std::sync::Arc;
use uuid::Uuid;

async fn connect_and_prepare(database: &DatabaseConfig) -> Result<DatabaseConnection, String> {
    let connection = connect(database).await.map_err(|error| error.to_string())?;
    prepare_schema(&connection, &database.schema)
        .await
        .map_err(|error| error.to_string())?;
    Migrator::up(&connection, None)
        .await
        .map_err(|error| error.to_string())?;

    Ok(connection)
}

#[async_trait]
trait AdminDatabaseConnector: Send + Sync {
    async fn connect(
        &self,
        database: &DatabaseConfig,
    ) -> Result<DatabaseConnection, AdminSetupError>;
}

type SharedAdminDatabaseConnector = Arc<dyn AdminDatabaseConnector>;

#[derive(Debug)]
struct PreparedPostgresAdminDatabaseConnector;

#[async_trait]
impl AdminDatabaseConnector for PreparedPostgresAdminDatabaseConnector {
    async fn connect(
        &self,
        database: &DatabaseConfig,
    ) -> Result<DatabaseConnection, AdminSetupError> {
        connect_and_prepare(database)
            .await
            .map_err(AdminSetupError::internal)
    }
}

pub fn default_database_initializer() -> SharedDatabaseInitializer {
    Arc::new(PostgresDatabaseInitializer)
}

#[derive(Debug)]
struct PostgresDatabaseInitializer;

#[async_trait]
impl DatabaseInitializer for PostgresDatabaseInitializer {
    async fn initialize_database(
        &self,
        database: &DatabaseConfig,
    ) -> Result<(), DatabaseSetupError> {
        connect_and_prepare(database)
            .await
            .map(|_| ())
            .map_err(DatabaseSetupError::new)
    }
}

pub fn default_setup_bootstrapper() -> SharedSetupBootstrapper {
    Arc::new(PostgresSetupBootstrapper)
}

#[derive(Debug)]
struct PostgresSetupBootstrapper;

#[async_trait]
impl SetupBootstrapper for PostgresSetupBootstrapper {
    async fn initialize_and_has_admin(
        &self,
        database: &DatabaseConfig,
    ) -> Result<bool, SetupBootstrapError> {
        let connection = connect_and_prepare(database)
            .await
            .map_err(SetupBootstrapError::new)?;

        SeaOrmUserRepository::new(connection)
            .has_admin()
            .await
            .map_err(|error| SetupBootstrapError::new(error.to_string()))
    }
}

pub struct PostgresInitialAdminCreator {
    database: DatabaseConfig,
    connector: SharedAdminDatabaseConnector,
}

impl PostgresInitialAdminCreator {
    pub fn new(database: DatabaseConfig) -> Self {
        Self::with_connector(database, Arc::new(PreparedPostgresAdminDatabaseConnector))
    }

    fn with_connector(database: DatabaseConfig, connector: SharedAdminDatabaseConnector) -> Self {
        Self {
            database,
            connector,
        }
    }
}

impl std::fmt::Debug for PostgresInitialAdminCreator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresInitialAdminCreator")
            .field("database", &self.database)
            .finish_non_exhaustive()
    }
}

fn hash_password(password: &str) -> Result<String, AdminSetupError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| AdminSetupError::internal(error.to_string()))
}

#[async_trait]
impl InitialAdminCreator for PostgresInitialAdminCreator {
    async fn create_initial_admin(&self, input: InitialAdminInput) -> Result<(), AdminSetupError> {
        let connection = self.connector.connect(&self.database).await?;
        let transaction = connection
            .begin()
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?;

        transaction
            .execute(Statement::from_string(
                DbBackend::Postgres,
                "SELECT pg_advisory_xact_lock(2026041701)".to_owned(),
            ))
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?;

        if user::Entity::find()
            .filter(user::Column::Email.eq(input.email.clone()))
            .one(&transaction)
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?
            .is_some()
        {
            return Err(AdminSetupError::conflict("email already exists"));
        }

        if user::Entity::find()
            .filter(user::Column::IsAdmin.eq(true))
            .one(&transaction)
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?
            .is_some()
        {
            return Err(AdminSetupError::conflict("initial admin already exists"));
        }

        let password_hash = hash_password(&input.password)?;
        let now = Utc::now();
        let user_id = Uuid::new_v4();

        let user_model = user::Model {
            id: user_id,
            email: input.email,
            is_admin: true,
            is_initial_admin: true,
            created_at: now,
            updated_at: now,
        };

        insert_user(&transaction, &user_model)
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?;

        upsert_password_credential(
            &transaction,
            &user_password_credential::Model {
                user_id,
                password_hash,
                password_updated_at: now,
            },
        )
        .await
        .map_err(|error| AdminSetupError::internal(error.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_worker_database::entities::user;
    use sea_orm::{
        DatabaseBackend, DatabaseConnection, MockDatabase, MockDatabaseConnection, MockExecResult,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct RecordingAdminDatabaseConnector {
        connection: Arc<MockDatabaseConnection>,
        last_database: Mutex<Option<DatabaseConfig>>,
    }

    impl RecordingAdminDatabaseConnector {
        fn new(connection: Arc<MockDatabaseConnection>) -> Self {
            Self {
                connection,
                last_database: Mutex::new(None),
            }
        }

        fn last_database(&self) -> Option<DatabaseConfig> {
            self.last_database.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AdminDatabaseConnector for RecordingAdminDatabaseConnector {
        async fn connect(
            &self,
            database: &DatabaseConfig,
        ) -> Result<DatabaseConnection, AdminSetupError> {
            *self.last_database.lock().unwrap() = Some(database.clone());
            Ok(DatabaseConnection::MockDatabaseConnection(
                self.connection.clone(),
            ))
        }
    }

    #[test]
    fn hash_password_uses_argon2id_encoding() {
        let hash = hash_password("correct horse battery staple").unwrap();

        assert!(hash.starts_with("$argon2id$"));
    }

    #[tokio::test]
    async fn create_initial_admin_uses_schema_prepared_connection() {
        let connection = Arc::new(MockDatabaseConnection::new(
            MockDatabase::new(DatabaseBackend::Postgres)
                .append_exec_results([
                    MockExecResult {
                        last_insert_id: 0,
                        rows_affected: 1,
                    },
                    MockExecResult {
                        last_insert_id: 0,
                        rows_affected: 1,
                    },
                    MockExecResult {
                        last_insert_id: 0,
                        rows_affected: 1,
                    },
                ])
                .append_query_results([Vec::<user::Model>::new(), Vec::<user::Model>::new()]),
        ));
        let connector = Arc::new(RecordingAdminDatabaseConnector::new(connection.clone()));
        let database = DatabaseConfig {
            schema: "control_plane".to_owned(),
            ..DatabaseConfig::default()
        };
        let creator =
            PostgresInitialAdminCreator::with_connector(database.clone(), connector.clone());

        creator
            .create_initial_admin(InitialAdminInput {
                email: "admin@example.com".to_owned(),
                password: "secret-pass".to_owned(),
            })
            .await
            .unwrap();

        assert_eq!(connector.last_database(), Some(database));

        let transaction_log =
            DatabaseConnection::MockDatabaseConnection(connection).into_transaction_log();
        assert_eq!(transaction_log.len(), 1);
        assert!(
            transaction_log[0]
                .statements()
                .iter()
                .any(|statement| statement.sql.contains("pg_advisory_xact_lock"))
        );
    }
}
