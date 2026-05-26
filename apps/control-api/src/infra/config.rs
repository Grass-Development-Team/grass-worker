use std::{net::IpAddr, path::Path};

use anyhow::Context;
use grass_config::{ConfigError, load_toml_or_default, overlay_bool, overlay_string, save_toml};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub format: LogFormat,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: LogFormat::Pretty,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: IpAddr,
    #[serde(default = "default_api_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_api_port(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedisConfig {
    #[serde(default = "default_redis_url")]
    pub url: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            url: default_redis_url(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_root")]
    pub root: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: default_storage_root(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecretsConfig {
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            secret_key: default_secret_key(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionConfig {
    #[serde(default)]
    pub cookie_secure: bool,
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            cookie_secure: false,
            session_ttl_seconds: default_session_ttl_seconds(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeManagerConfig {
    #[serde(default)]
    pub auto_start_local_node: bool,
    #[serde(default = "default_local_node_binary")]
    pub local_node_binary: String,
    #[serde(default = "default_local_node_config")]
    pub local_node_config: String,
    #[serde(default = "default_restart_on_exit")]
    pub restart_on_exit: bool,
}

impl Default for NodeManagerConfig {
    fn default() -> Self {
        Self {
            auto_start_local_node: false,
            local_node_binary: default_local_node_binary(),
            local_node_config: default_local_node_config(),
            restart_on_exit: true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationConfig {
    #[serde(default)]
    pub auto_migrate: bool,
}

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
            LogFormat::Pretty => subscriber
                .try_init()
                .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
            LogFormat::Json => subscriber
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

fn default_host() -> IpAddr {
    IpAddr::from([127, 0, 0, 1])
}

const fn default_api_port() -> u16 {
    8080
}

fn default_log_level() -> String {
    "info".to_owned()
}

fn default_database_url() -> String {
    String::new()
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/0".to_owned()
}

fn default_storage_root() -> String {
    "/data".to_owned()
}

fn default_secret_key() -> String {
    "change-me".to_owned()
}

const fn default_session_ttl_seconds() -> u64 {
    2_592_000
}

fn default_local_node_binary() -> String {
    "grass-node".to_owned()
}

fn default_local_node_config() -> String {
    "./node.toml".to_owned()
}

const fn default_restart_on_exit() -> bool {
    true
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
