use std::{net::IpAddr, path::Path};

use anyhow::Context;
use serde::Deserialize;
use thiserror::Error;
use tracing_subscriber::{EnvFilter, fmt};

mod env;
mod file;
mod validation;

pub use validation::Validate;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration: {0}")]
    Load(#[from] config::ConfigError),
    #[error("invalid environment variable {name}: {source}")]
    Env {
        name: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("invalid {field}: {message}")]
    Validation {
        field: &'static str,
        message: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Default)]
pub struct MigrationConfig {
    #[serde(default)]
    pub auto_migrate: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Default)]
pub struct ControlApiConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub migration: MigrationConfig,
    #[serde(default)]
    pub log: LogConfig,
}

impl ControlApiConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut config = file::load_toml_or_default(path)?;
        env::apply_api_env(&mut config)?;
        config.validate()?;
        Ok(config)
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

impl Validate for ControlApiConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        self.database.validate()
    }
}

impl Validate for DatabaseConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        let url = url::Url::parse(&self.url).map_err(|error| ConfigError::Validation {
            field: "database.url",
            message: error.to_string(),
        })?;

        match url.scheme() {
            "postgres" | "postgresql" => Ok(()),
            scheme => Err(ConfigError::Validation {
                field: "database.url",
                message: format!("unsupported scheme: {scheme}"),
            }),
        }
    }
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
    "postgres://postgres:postgres@127.0.0.1:5432/grass_worker".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_are_safe_for_local_bootstrap() {
        let cfg = ControlApiConfig::default();
        assert_eq!(cfg.server.port, 8080);
        assert_eq!(cfg.log.level, "info");
        assert!(!cfg.migration.auto_migrate);
    }

    #[test]
    fn database_config_rejects_non_postgres_urls() {
        let cfg = DatabaseConfig {
            url: "redis://127.0.0.1:6379/0".to_owned(),
        };
        let error = cfg.validate().unwrap_err();
        assert!(error.to_string().contains("unsupported scheme"));
    }
}
