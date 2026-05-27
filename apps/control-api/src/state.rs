use std::sync::{Arc, OnceLock, RwLock};

use redis::aio::MultiplexedConnection;
use sea_orm::DatabaseConnection;

use crate::infra::config::ControlApiConfig;

#[derive(Clone)]
pub struct ControlApiState {
    pub config: Arc<RwLock<ControlApiConfig>>,
    config_path: Arc<String>,
    pub database: Arc<OnceLock<DatabaseConnection>>,
    pub redis: Arc<OnceLock<MultiplexedConnection>>,
}

impl ControlApiState {
    pub fn new(config: ControlApiConfig, config_path: impl Into<String>) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path: Arc::new(config_path.into()),
            database: Arc::new(OnceLock::new()),
            redis: Arc::new(OnceLock::new()),
        }
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    pub fn try_database(&self) -> Option<&DatabaseConnection> {
        self.database.get()
    }

    pub fn try_redis(&self) -> Option<&MultiplexedConnection> {
        self.redis.get()
    }
}
