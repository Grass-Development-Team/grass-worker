use grass_worker_config::AppConfig;
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupContext {
    pub listen: SocketAddr,
    pub stage: SetupStage,
}

impl SetupContext {
    pub fn database(listen: SocketAddr) -> Self {
        Self {
            listen,
            stage: SetupStage::Database,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Ready(AppConfig),
    Setup(SetupContext),
}

impl StartupMode {
    pub fn from_api_config(config: AppConfig) -> Self {
        if config.database.is_some() {
            Self::Ready(config)
        } else {
            Self::Setup(SetupContext::database(config.server.listen))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grass_worker_config::AppConfig;

    #[test]
    fn from_api_config_uses_setup_mode_when_database_is_missing() {
        let mode = StartupMode::from_api_config(AppConfig::defaults());

        match mode {
            StartupMode::Setup(context) => {
                assert_eq!(context.stage, SetupStage::Database);
            }
            StartupMode::Ready(_) => panic!("expected setup mode when database config is missing"),
        }
    }

    #[test]
    fn from_api_config_uses_ready_mode_when_database_is_present() {
        let mode = StartupMode::from_api_config(AppConfig {
            server: grass_worker_config::ServerConfig::default(),
            database: Some(grass_worker_config::DatabaseConfig::default()),
            development: None,
        });

        match mode {
            StartupMode::Ready(config) => {
                assert!(config.database.is_some());
            }
            StartupMode::Setup(_) => panic!("expected ready mode when database config is present"),
        }
    }
}
