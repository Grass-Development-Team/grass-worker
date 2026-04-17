use grass_worker_config::{AppConfig, DatabaseConfig, ResolvedApiConfig};
use grass_worker_database::{
    connection::{connect, prepare_schema},
    repository::{SeaOrmUserRepository, UserRepository},
};
use grass_worker_migration::{Migrator, MigratorTrait};
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
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

#[async_trait::async_trait]
pub trait DatabaseSetupService: Send + Sync {
    async fn initialize_database(
        &self,
        database: &DatabaseConfig,
    ) -> Result<(), DatabaseSetupError>;
}

pub type SharedDatabaseSetupService = Arc<dyn DatabaseSetupService>;

#[derive(Debug)]
struct LiveDatabaseSetupService;

#[async_trait::async_trait]
impl DatabaseSetupService for LiveDatabaseSetupService {
    async fn initialize_database(
        &self,
        database: &DatabaseConfig,
    ) -> Result<(), DatabaseSetupError> {
        let connection = connect(database)
            .await
            .map_err(|error| DatabaseSetupError::new(error.to_string()))?;
        prepare_schema(&connection, &database.schema)
            .await
            .map_err(|error| DatabaseSetupError::new(error.to_string()))?;
        Migrator::up(&connection, None)
            .await
            .map_err(|error| DatabaseSetupError::new(error.to_string()))?;

        Ok(())
    }
}

fn default_database_setup_service() -> SharedDatabaseSetupService {
    Arc::new(LiveDatabaseSetupService)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupModeError {
    message: String,
}

impl StartupModeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StartupModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StartupModeError {}

#[async_trait::async_trait]
pub trait StartupDatabaseService: Send + Sync {
    async fn initialize_and_has_admin(
        &self,
        database: &DatabaseConfig,
    ) -> Result<bool, StartupModeError>;
}

type SharedStartupDatabaseService = Arc<dyn StartupDatabaseService>;

#[derive(Debug)]
struct LiveStartupDatabaseService;

#[async_trait::async_trait]
impl StartupDatabaseService for LiveStartupDatabaseService {
    async fn initialize_and_has_admin(
        &self,
        database: &DatabaseConfig,
    ) -> Result<bool, StartupModeError> {
        let connection = connect(database)
            .await
            .map_err(|error| StartupModeError::new(error.to_string()))?;
        prepare_schema(&connection, &database.schema)
            .await
            .map_err(|error| StartupModeError::new(error.to_string()))?;
        Migrator::up(&connection, None)
            .await
            .map_err(|error| StartupModeError::new(error.to_string()))?;

        SeaOrmUserRepository::new(connection)
            .has_admin()
            .await
            .map_err(|error| StartupModeError::new(error.to_string()))
    }
}

fn default_startup_database_service() -> SharedStartupDatabaseService {
    Arc::new(LiveStartupDatabaseService)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    Database,
    Admin,
}

#[derive(Clone)]
pub enum SetupContext {
    Database {
        listen: SocketAddr,
        config_path: PathBuf,
        service: SharedDatabaseSetupService,
    },
    Admin {
        listen: SocketAddr,
        database: DatabaseConfig,
    },
}

impl std::fmt::Debug for SetupContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database {
                listen,
                config_path,
                ..
            } => f
                .debug_struct("SetupContext::Database")
                .field("listen", listen)
                .field("config_path", config_path)
                .finish_non_exhaustive(),
            Self::Admin { listen, database } => f
                .debug_struct("SetupContext::Admin")
                .field("listen", listen)
                .field("database", database)
                .finish_non_exhaustive(),
        }
    }
}

impl PartialEq for SetupContext {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Database {
                    listen: left_listen,
                    config_path: left_config_path,
                    ..
                },
                Self::Database {
                    listen: right_listen,
                    config_path: right_config_path,
                    ..
                },
            ) => left_listen == right_listen && left_config_path == right_config_path,
            (
                Self::Admin {
                    listen: left_listen,
                    database: left_database,
                },
                Self::Admin {
                    listen: right_listen,
                    database: right_database,
                },
            ) => left_listen == right_listen && left_database == right_database,
            _ => false,
        }
    }
}

impl Eq for SetupContext {}

impl SetupContext {
    pub fn database(listen: SocketAddr, config_path: PathBuf) -> Self {
        Self::database_with_service(listen, config_path, default_database_setup_service())
    }

    pub fn database_with_service(
        listen: SocketAddr,
        config_path: PathBuf,
        service: SharedDatabaseSetupService,
    ) -> Self {
        Self::Database {
            listen,
            config_path,
            service,
        }
    }

    pub fn admin(listen: SocketAddr, database: DatabaseConfig) -> Self {
        Self::Admin { listen, database }
    }

    pub fn stage(&self) -> SetupStage {
        match self {
            Self::Database { .. } => SetupStage::Database,
            Self::Admin { .. } => SetupStage::Admin,
        }
    }

    pub fn listen(&self) -> SocketAddr {
        match self {
            Self::Database { listen, .. } | Self::Admin { listen, .. } => *listen,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Ready(AppConfig),
    Setup(SetupContext),
}

impl StartupMode {
    pub async fn resolve(resolved: ResolvedApiConfig) -> Result<Self, StartupModeError> {
        Self::resolve_with_service(resolved, default_startup_database_service()).await
    }

    pub async fn resolve_with_service(
        resolved: ResolvedApiConfig,
        service: SharedStartupDatabaseService,
    ) -> Result<Self, StartupModeError> {
        let Some(database) = resolved.config.database.clone() else {
            return Ok(Self::Setup(SetupContext::database(
                resolved.config.server.listen,
                resolved.path,
            )));
        };

        if service.initialize_and_has_admin(&database).await? {
            Ok(Self::Ready(resolved.config))
        } else {
            Ok(Self::Setup(SetupContext::admin(
                resolved.config.server.listen,
                database,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_worker_config::{AppConfig, DatabaseConfig, ResolvedApiConfig};
    use std::path::PathBuf;
    use std::sync::Arc;

    #[derive(Debug)]
    struct StubStartupDatabaseService {
        has_admin: bool,
    }

    #[async_trait::async_trait]
    impl StartupDatabaseService for StubStartupDatabaseService {
        async fn initialize_and_has_admin(
            &self,
            _database: &DatabaseConfig,
        ) -> Result<bool, StartupModeError> {
            Ok(self.has_admin)
        }
    }

    #[tokio::test]
    async fn resolve_uses_setup_mode_when_database_is_missing() {
        let mode = StartupMode::resolve_with_service(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig::defaults(),
            },
            Arc::new(StubStartupDatabaseService { has_admin: false }),
        )
        .await
        .unwrap();

        match mode {
            StartupMode::Setup(context) => {
                assert_eq!(context.stage(), SetupStage::Database);
            }
            StartupMode::Ready(_) => panic!("expected setup mode when database config is missing"),
        }
    }

    #[tokio::test]
    async fn resolve_uses_admin_stage_when_database_exists_without_admin() {
        let mode = StartupMode::resolve_with_service(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig {
                    server: grass_worker_config::ServerConfig::default(),
                    database: Some(DatabaseConfig::default()),
                    development: None,
                },
            },
            Arc::new(StubStartupDatabaseService { has_admin: false }),
        )
        .await
        .unwrap();

        match mode {
            StartupMode::Setup(context) => {
                assert_eq!(context.stage(), SetupStage::Admin);
            }
            StartupMode::Ready(_) => panic!("expected admin setup mode"),
        }
    }

    #[tokio::test]
    async fn resolve_uses_ready_mode_when_admin_exists() {
        let mode = StartupMode::resolve_with_service(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig {
                    server: grass_worker_config::ServerConfig::default(),
                    database: Some(DatabaseConfig::default()),
                    development: None,
                },
            },
            Arc::new(StubStartupDatabaseService { has_admin: true }),
        )
        .await
        .unwrap();

        match mode {
            StartupMode::Ready(config) => {
                assert!(config.database.is_some());
            }
            StartupMode::Setup(_) => panic!("expected ready mode when database config is present"),
        }
    }
}
