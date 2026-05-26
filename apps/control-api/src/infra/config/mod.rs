pub mod database;
pub mod log;
pub mod migration;
pub mod node_manager;
pub mod redis;
pub mod secrets;
pub mod server;
pub mod session;
pub mod storage;

use std::path::Path;

use anyhow::Context;
use grass_config::{ConfigError, load_toml_or_default, overlay_bool, overlay_string, save_toml};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt};

use self::{
    database::DatabaseConfig, log::LogConfig, migration::MigrationConfig,
    node_manager::NodeManagerConfig, redis::RedisConfig, secrets::SecretsConfig,
    server::ServerConfig, session::SessionConfig, storage::StorageConfig,
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ControlApiConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub redis: RedisConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub secrets: SecretsConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub node_manager: NodeManagerConfig,
    #[serde(default)]
    pub migration: MigrationConfig,
    #[serde(default)]
    pub log: LogConfig,
}

impl ControlApiConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut config = load_toml_or_default(path)?;
        apply_env(&mut config)?;
        Ok(config)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        save_toml(path, self)
    }

    pub fn init_tracing(&self) -> anyhow::Result<()> {
        let filter = EnvFilter::try_new(&self.log.level).context("invalid tracing filter")?;
        let subscriber = fmt().with_env_filter(filter);

        match self.log.format {
            log::LogFormat::Pretty => subscriber
                .try_init()
                .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
            log::LogFormat::Json => subscriber
                .json()
                .try_init()
                .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
        }

        Ok(())
    }
}

fn apply_env(config: &mut ControlApiConfig) -> Result<(), ConfigError> {
    overlay_string("GWAPI_DATABASE_URL", &mut config.database.url);
    overlay_string("GWAPI_REDIS_URL", &mut config.redis.url);
    overlay_string("GWAPI_STORAGE_ROOT", &mut config.storage.root);
    overlay_string("GWAPI_SECRET_KEY", &mut config.secrets.secret_key);
    overlay_bool(
        "GWAPI_NODE_MANAGER_AUTO_START_LOCAL_NODE",
        &mut config.node_manager.auto_start_local_node,
    )?;
    overlay_string(
        "GWAPI_NODE_MANAGER_LOCAL_NODE_BINARY",
        &mut config.node_manager.local_node_binary,
    );
    overlay_string(
        "GWAPI_NODE_MANAGER_LOCAL_NODE_CONFIG",
        &mut config.node_manager.local_node_config,
    );
    overlay_string("GWAPI_LOG_LEVEL", &mut config.log.level);
    overlay_string("LOG_LEVEL", &mut config.log.level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_database_url_is_empty_for_setup() {
        let cfg = ControlApiConfig::default();
        assert!(cfg.database.url.is_empty());
    }
}
