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

#[derive(Debug)]
pub struct PostgresInitialAdminCreator {
    database: DatabaseConfig,
}

impl PostgresInitialAdminCreator {
    pub fn new(database: DatabaseConfig) -> Self {
        Self { database }
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
        let connection = connect(&self.database)
            .await
            .map_err(|error| AdminSetupError::internal(error.to_string()))?;
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

    #[test]
    fn hash_password_uses_argon2id_encoding() {
        let hash = hash_password("correct horse battery staple").unwrap();

        assert!(hash.starts_with("$argon2id$"));
    }
}
