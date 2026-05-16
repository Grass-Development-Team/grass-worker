use std::env;

use super::{ConfigError, ControlApiConfig};

pub fn apply_api_env(config: &mut ControlApiConfig) -> Result<(), ConfigError> {
    overlay_string("GWAPI_DATABASE_URL", &mut config.database.url);
    overlay_string("GWAPI_LOG_LEVEL", &mut config.log.level);
    overlay_string("GWAPI_SERVER_HOST", &mut config.server.host);
    overlay_u16("GWAPI_SERVER_PORT", &mut config.server.port)?;
    overlay_bool(
        "GWAPI_MIGRATION_AUTO_MIGRATE",
        &mut config.migration.auto_migrate,
    )?;
    overlay_common_log_level(&mut config.log.level);
    Ok(())
}

fn overlay_common_log_level(target: &mut String) {
    overlay_string("LOG_LEVEL", target);
}

fn overlay_string<T>(name: &'static str, target: &mut T)
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    if let Ok(value) = env::var(name) {
        if let Ok(value) = value.parse() {
            *target = value;
        }
    }
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

fn overlay_bool(name: &'static str, target: &mut bool) -> Result<(), ConfigError> {
    if let Ok(value) = env::var(name) {
        *target = value.parse().map_err(|source| ConfigError::Env {
            name,
            source: Box::new(source),
        })?;
    }
    Ok(())
}
