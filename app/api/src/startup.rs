use grass_worker_config::{AppConfig, DatabaseConfig, ResolvedApiConfig};
use grass_worker_database::connection::{connect, prepare_schema};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    Database,
}

#[derive(Clone)]
pub struct SetupContext {
    pub listen: SocketAddr,
    pub stage: SetupStage,
    pub config_path: PathBuf,
    pub service: SharedDatabaseSetupService,
}

impl std::fmt::Debug for SetupContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupContext")
            .field("listen", &self.listen)
            .field("stage", &self.stage)
            .field("config_path", &self.config_path)
            .finish_non_exhaustive()
    }
}

impl PartialEq for SetupContext {
    fn eq(&self, other: &Self) -> bool {
        self.listen == other.listen
            && self.stage == other.stage
            && self.config_path == other.config_path
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
        Self {
            listen,
            stage: SetupStage::Database,
            config_path,
            service,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Ready(AppConfig),
    Setup(SetupContext),
}

impl StartupMode {
    pub fn from_api_config(resolved: ResolvedApiConfig) -> Self {
        if resolved.config.database.is_some() {
            Self::Ready(resolved.config)
        } else {
            Self::Setup(SetupContext::database(
                resolved.config.server.listen,
                resolved.path,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_worker_config::{AppConfig, ResolvedApiConfig};
    use std::path::PathBuf;

    #[test]
    fn from_api_config_uses_setup_mode_when_database_is_missing() {
        let mode = StartupMode::from_api_config(ResolvedApiConfig {
            path: PathBuf::from("config.toml"),
            config: AppConfig::defaults(),
        });

        match mode {
            StartupMode::Setup(context) => {
                assert_eq!(context.stage, SetupStage::Database);
                assert_eq!(context.config_path, PathBuf::from("config.toml"));
            }
            StartupMode::Ready(_) => panic!("expected setup mode when database config is missing"),
        }
    }

    #[test]
    fn from_api_config_uses_ready_mode_when_database_is_present() {
        let mode = StartupMode::from_api_config(ResolvedApiConfig {
            path: PathBuf::from("config.toml"),
            config: AppConfig {
                server: grass_worker_config::ServerConfig::default(),
                database: Some(grass_worker_config::DatabaseConfig::default()),
                development: None,
            },
        });

        match mode {
            StartupMode::Ready(config) => {
                assert!(config.database.is_some());
            }
            StartupMode::Setup(_) => panic!("expected ready mode when database config is present"),
        }
    }
}
