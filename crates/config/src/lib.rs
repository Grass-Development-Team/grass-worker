//! Shared configuration helpers.

use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use config::{Config, File};
use serde::{Serialize, de::DeserializeOwned};
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
    #[error("invalid configuration: {0}")]
    Invalid(String),
    #[error("failed to serialize configuration: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("failed to write configuration: {0}")]
    Write(#[from] std::io::Error),
}

pub fn load_toml_or_default<T>(path: impl AsRef<Path>) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    let cfg = Config::builder()
        .add_source(File::from(path.as_ref()).required(false))
        .build()?;

    Ok(cfg.try_deserialize()?)
}

pub fn save_toml<T>(path: impl AsRef<Path>, value: &T) -> Result<(), ConfigError>
where
    T: Serialize,
{
    let path = path.as_ref();
    let content = toml::to_string_pretty(value)?;
    let temporary_path = temporary_path(path);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary_path, path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    write_result?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.toml");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        unique
    ))
}

pub fn overlay_string(name: &'static str, target: &mut String) {
    if let Ok(value) = env::var(name) {
        *target = value;
    }
}

pub fn overlay_bool(name: &'static str, target: &mut bool) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

pub fn overlay_u16(name: &'static str, target: &mut u16) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

pub fn overlay_u64(name: &'static str, target: &mut u64) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, Deserialize, PartialEq, Serialize)]
    struct ExampleConfig {
        #[serde(default)]
        name: String,
    }

    #[test]
    fn missing_file_uses_type_defaults() {
        let cfg: ExampleConfig = super::load_toml_or_default("missing-config.toml").unwrap();
        assert_eq!(cfg, ExampleConfig::default());
    }
}
