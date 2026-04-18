use argon2::{
    Argon2,
    password_hash::{PasswordHasher, SaltString},
};
use async_trait::async_trait;
use chrono::Utc;
use grass_worker_config::DatabaseConfig;
use grass_worker_database::{
    connection::connect,
    entities::{user, user_password_credential},
    repository::{insert_user, upsert_password_credential},
};
use rand_core::OsRng;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, QueryFilter, Statement,
    TransactionTrait,
};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialAdminInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminSetupErrorKind {
    Validation,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSetupError {
    kind: AdminSetupErrorKind,
    message: String,
}

impl AdminSetupError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &AdminSetupErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait]
pub trait AdminSetupService: Send + Sync {
    async fn create_initial_admin(
        &self,
        input: InitialAdminInput,
    ) -> Result<(), AdminSetupError>;
}

pub type SharedAdminSetupService = Arc<dyn AdminSetupService>;

#[derive(Debug)]
pub struct LiveAdminSetupService {
    database: DatabaseConfig,
}

impl LiveAdminSetupService {
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
impl AdminSetupService for LiveAdminSetupService {
    async fn create_initial_admin(
        &self,
        input: InitialAdminInput,
    ) -> Result<(), AdminSetupError> {
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
