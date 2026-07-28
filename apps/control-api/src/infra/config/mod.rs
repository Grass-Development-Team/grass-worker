pub mod cache;
pub mod database;
pub mod log;
pub mod migration;
pub mod node_manager;
pub mod secrets;
pub mod server;
pub mod session;
pub mod storage;

use std::{env, net::SocketAddr, path::Path};

use anyhow::Context;
use grass_config::{ConfigError, load_toml_or_default, overlay_bool, overlay_string, save_toml};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt};

use self::{
    cache::CacheConfig, database::DatabaseConfig, log::LogConfig, migration::MigrationConfig,
    node_manager::NodeManagerConfig, secrets::SecretsConfig, server::ServerConfig,
    session::SessionConfig, storage::StorageConfig,
};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ControlApiConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default, alias = "cache")]
    pub redis: CacheConfig,
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
        let path = path.as_ref();
        let config_exists = path.exists();
        let mut config = Self::load_persisted(path)?;
        if !config_exists {
            config.ensure_secret_key();
            config.save(path)?;
        }
        apply_env(&mut config)?;
        config.ensure_secret_key();
        Ok(config)
    }

    pub fn load_persisted(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        load_toml_or_default(path)
    }

    pub fn ensure_secret_key(&mut self) -> bool {
        if secret_key_is_strong(&self.secrets.secret_key) {
            return false;
        }

        self.secrets.secret_key = grass_token::generate_token();
        true
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

fn secret_key_is_strong(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value != "change-me" && value.len() >= 32
}

fn apply_env(config: &mut ControlApiConfig) -> Result<(), ConfigError> {
    if let Ok(value) = env::var("GWAPI_SERVER_LISTEN") {
        let listen = parse_listen("GWAPI_SERVER_LISTEN", &value)?;
        config.server.host = listen.ip();
        config.server.port = listen.port();
    }
    overlay_string("GWAPI_DATABASE_URL", &mut config.database.url);
    overlay_string("GWAPI_CACHE_REDIS_URL", &mut config.redis.url);
    overlay_string("GWAPI_REDIS_URL", &mut config.redis.url);
    overlay_string("GWAPI_STORAGE_ROOT", &mut config.storage.root);
    overlay_string("GWAPI_SECRET_KEY", &mut config.secrets.secret_key);
    if let Ok(master_key) = env::var("GWAPI_GIT_CREDENTIAL_MASTER_KEY") {
        let key_id = env::var("GWAPI_GIT_CREDENTIAL_KEY_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "primary".to_owned());
        config.secrets.git_credentials.active_key_id = key_id.clone();
        config
            .secrets
            .git_credentials
            .keys
            .insert(key_id, master_key);
    }
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

fn parse_listen(name: &'static str, value: &str) -> Result<SocketAddr, ConfigError> {
    value.parse().map_err(|source| ConfigError::Env {
        name,
        source: Box::new(source),
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;

    #[test]
    fn default_database_url_is_empty_for_setup() {
        let cfg = ControlApiConfig::default();
        assert!(cfg.database.url.is_empty());
    }

    #[test]
    fn documented_redis_section_enables_redis_backend() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grass-worker-config-{unique}.toml"));
        fs::write(&path, "[redis]\nurl = \"redis://cache.example/2\"\n").unwrap();

        let config = ControlApiConfig::load(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(config.redis.backend, grass_cache::CacheBackend::Redis);
        assert_eq!(config.redis.url, "redis://cache.example/2");
    }

    #[test]
    fn loading_an_existing_config_does_not_rewrite_it() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grass-worker-config-{unique}.toml"));
        let original = "# Keep operator edits intact.\n[redis]\nurl = \"redis://cache.example/2\"\n\n[secrets]\nsecret_key = \"change-me\"\n";
        fs::write(&path, original).unwrap();

        let config = ControlApiConfig::load(&path).unwrap();
        let persisted = fs::read_to_string(&path).unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(config.redis.url, "redis://cache.example/2");
        assert_eq!(persisted, original);
    }

    #[test]
    fn weak_secret_key_is_replaced_once() {
        let mut config = ControlApiConfig::default();
        assert!(config.ensure_secret_key());
        assert_ne!(config.secrets.secret_key, "change-me");
        assert!(config.secrets.secret_key.len() >= 43);
        let generated = config.secrets.secret_key.clone();

        assert!(!config.ensure_secret_key());
        assert_eq!(config.secrets.secret_key, generated);
    }

    #[test]
    fn server_listen_uses_socket_address_format() {
        let listen = parse_listen("GWAPI_SERVER_LISTEN", "0.0.0.0:7817").unwrap();
        assert_eq!(listen.ip().to_string(), "0.0.0.0");
        assert_eq!(listen.port(), 7817);
        assert!(parse_listen("GWAPI_SERVER_LISTEN", "localhost").is_err());
    }
}
