//! Bootstrap configuration for grass-worker binaries.

use std::{env, net::IpAddr, path::Path};

use config::{Config, File};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] config::ConfigError),
    #[error("invalid environment variable {name}: {source}")]
    Env {
        name: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct MigrationConfig {
    #[serde(default)]
    pub auto_migrate: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct NodeIdentityConfig {
    #[serde(default = "default_node_id")]
    pub id: String,
    #[serde(default = "default_control_api")]
    pub control_api: String,
    #[serde(default = "default_node_token")]
    pub node_token: String,
    #[serde(default = "default_node_work_root")]
    pub work_root: String,
    #[serde(default)]
    pub capabilities: NodeCapabilitiesConfig,
}

impl Default for NodeIdentityConfig {
    fn default() -> Self {
        Self {
            id: default_node_id(),
            control_api: default_control_api(),
            node_token: default_node_token(),
            work_root: default_node_work_root(),
            capabilities: NodeCapabilitiesConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct NodeCapabilitiesConfig {
    #[serde(default = "default_true")]
    pub build: bool,
    #[serde(default = "default_true")]
    pub serve: bool,
}

impl Default for NodeCapabilitiesConfig {
    fn default() -> Self {
        Self {
            build: true,
            serve: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BuildConfig {
    #[serde(default = "default_build_concurrency")]
    pub concurrency: u16,
    #[serde(default = "default_build_timeout_seconds")]
    pub command_timeout_seconds: u64,
    #[serde(default)]
    pub retain_workspace_on_failure: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            concurrency: default_build_concurrency(),
            command_timeout_seconds: default_build_timeout_seconds(),
            retain_workspace_on_failure: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ServeConfig {
    #[serde(default = "default_serve_host")]
    pub host: IpAddr,
    #[serde(default = "default_serve_port")]
    pub port: u16,
    #[serde(default = "default_serve_public_base_url")]
    pub public_base_url: String,
    #[serde(default = "default_metadata_cache_ttl_seconds")]
    pub metadata_cache_ttl_seconds: u64,
    #[serde(default = "default_artifact_cache_root")]
    pub artifact_cache_root: String,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            host: default_serve_host(),
            port: default_serve_port(),
            public_base_url: default_serve_public_base_url(),
            metadata_cache_ttl_seconds: default_metadata_cache_ttl_seconds(),
            artifact_cache_root: default_artifact_cache_root(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct SecurityConfig {
    #[serde(default)]
    pub allow_private_repository: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub verbose_build_log: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct NodeConfig {
    #[serde(default)]
    pub node: NodeIdentityConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub serve: ServeConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub development: DevelopmentConfig,
    #[serde(default)]
    pub log: LogConfig,
}

impl ControlApiConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut config = load_toml_or_default(path)?;
        apply_api_env(&mut config)?;
        Ok(config)
    }
}

impl NodeConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut config = load_toml_or_default(path)?;
        apply_node_env(&mut config)?;
        Ok(config)
    }
}

fn load_toml_or_default<T>(path: impl AsRef<Path>) -> Result<T, ConfigError>
where
    T: for<'de> Deserialize<'de>,
{
    let cfg = Config::builder()
        .add_source(File::from(path.as_ref()).required(false))
        .build()?;

    Ok(cfg.try_deserialize()?)
}

fn apply_api_env(config: &mut ControlApiConfig) -> Result<(), ConfigError> {
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
    overlay_common_log_level(&mut config.log.level);
    Ok(())
}

fn apply_node_env(config: &mut NodeConfig) -> Result<(), ConfigError> {
    overlay_string("GWNODE_ID", &mut config.node.id);
    overlay_string("GWNODE_CONTROL_API", &mut config.node.control_api);
    overlay_string("GWNODE_NODE_TOKEN", &mut config.node.node_token);
    overlay_string("GWNODE_WORK_ROOT", &mut config.node.work_root);
    overlay_u16("GWNODE_BUILD_CONCURRENCY", &mut config.build.concurrency)?;
    overlay_u64(
        "GWNODE_BUILD_COMMAND_TIMEOUT_SECONDS",
        &mut config.build.command_timeout_seconds,
    )?;
    overlay_string(
        "GWNODE_SERVE_PUBLIC_BASE_URL",
        &mut config.serve.public_base_url,
    );
    overlay_string("GWNODE_LOG_LEVEL", &mut config.log.level);
    overlay_common_log_level(&mut config.log.level);
    Ok(())
}

fn overlay_common_log_level(target: &mut String) {
    overlay_string("LOG_LEVEL", target);
}

fn overlay_string(name: &'static str, target: &mut String) {
    if let Ok(value) = env::var(name) {
        *target = value;
    }
}

fn overlay_bool(name: &'static str, target: &mut bool) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

fn overlay_u16(name: &'static str, target: &mut u16) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

fn overlay_u64(name: &'static str, target: &mut u64) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

fn default_host() -> IpAddr {
    IpAddr::from([127, 0, 0, 1])
}

fn default_serve_host() -> IpAddr {
    IpAddr::from([0, 0, 0, 0])
}

const fn default_api_port() -> u16 {
    8080
}

const fn default_serve_port() -> u16 {
    8080
}

fn default_log_level() -> String {
    "info".to_owned()
}

fn default_database_url() -> String {
    "postgres://postgres:postgres@127.0.0.1:5432/grass_worker".to_owned()
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

fn default_node_id() -> String {
    "local-node".to_owned()
}

fn default_control_api() -> String {
    "http://127.0.0.1:8080".to_owned()
}

fn default_node_token() -> String {
    "change-me".to_owned()
}

fn default_node_work_root() -> String {
    "/data/node".to_owned()
}

const fn default_true() -> bool {
    true
}

const fn default_build_concurrency() -> u16 {
    1
}

const fn default_build_timeout_seconds() -> u64 {
    600
}

fn default_serve_public_base_url() -> String {
    "http://127.0.0.1:8080".to_owned()
}

const fn default_metadata_cache_ttl_seconds() -> u64 {
    30
}

fn default_artifact_cache_root() -> String {
    "/data/node/artifacts".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_api_defaults_are_safe_for_local_bootstrap() {
        let cfg = ControlApiConfig::default();
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.storage.root, "/data");
        assert_eq!(cfg.log.level, "info");
    }

    #[test]
    fn node_defaults_are_safe_for_local_bootstrap() {
        let cfg = NodeConfig::default();
        assert_eq!(cfg.node.id, "local-node");
        assert_eq!(cfg.node.control_api, "http://127.0.0.1:8080");
        assert_eq!(cfg.log.format, LogFormat::Pretty);
    }
}
