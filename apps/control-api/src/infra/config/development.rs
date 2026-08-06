use grass_config::ConfigError;
use serde::{Deserialize, Serialize};

const DEV_MODE_ENV: &str = "GWAPI_DEV_MODE";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub enabled: bool,
}

pub(super) fn apply_env_value(
    config: &mut DevelopmentConfig,
    value: Option<&str>,
) -> Result<(), ConfigError> {
    if let Some(value) = value {
        config.enabled = value.parse().map_err(|source| ConfigError::Env {
            name: DEV_MODE_ENV,
            source: Box::new(source),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_mode_defaults_disabled() {
        assert!(!DevelopmentConfig::default().enabled);
    }

    #[test]
    fn environment_value_overlays_file_configuration() {
        let mut config = DevelopmentConfig { enabled: true };

        apply_env_value(&mut config, Some("false")).unwrap();

        assert!(!config.enabled);

        apply_env_value(&mut config, Some("true")).unwrap();

        assert!(config.enabled);
    }
}
