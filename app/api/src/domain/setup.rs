use async_trait::async_trait;
use grass_worker_config::DatabaseConfig;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseSetupError {
    message: String,
}

impl DatabaseSetupError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for DatabaseSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DatabaseSetupError {}

#[async_trait]
pub trait DatabaseInitializer: Send + Sync {
    async fn initialize_database(
        &self,
        database: &DatabaseConfig,
    ) -> Result<(), DatabaseSetupError>;
}

pub type SharedDatabaseInitializer = Arc<dyn DatabaseInitializer>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupBootstrapError {
    message: String,
}

impl SetupBootstrapError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SetupBootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SetupBootstrapError {}

#[async_trait]
pub trait SetupBootstrapper: Send + Sync {
    async fn initialize_and_has_admin(
        &self,
        database: &DatabaseConfig,
    ) -> Result<bool, SetupBootstrapError>;
}

pub type SharedSetupBootstrapper = Arc<dyn SetupBootstrapper>;

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

impl std::fmt::Display for AdminSetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AdminSetupError {}

#[async_trait]
pub trait InitialAdminCreator: Send + Sync {
    async fn create_initial_admin(&self, input: InitialAdminInput) -> Result<(), AdminSetupError>;
}

pub type SharedInitialAdminCreator = Arc<dyn InitialAdminCreator>;
