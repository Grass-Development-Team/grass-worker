use std::sync::{Arc, OnceLock, RwLock};

use sea_orm::DatabaseConnection;

use crate::infra::{
    config::ControlApiConfig, logs::LogStreamHub, node_manager::NodeManager,
    storage::StorageManager,
};

#[derive(Clone)]
pub struct ControlApiState {
    pub config: Arc<RwLock<ControlApiConfig>>,
    config_path: Arc<String>,
    pub database: Arc<OnceLock<DatabaseConnection>>,
    pub cache: Arc<OnceLock<grass_cache::CacheStore>>,
    pub log_hub: LogStreamHub,
    pub node_manager: NodeManager,
    pub storage: StorageManager,
    setup_mutex: Arc<tokio::sync::Mutex<()>>,
}

impl ControlApiState {
    pub fn new(config: ControlApiConfig, config_path: impl Into<String>) -> Self {
        let node_manager = NodeManager::new(&config.node_manager);
        let storage = StorageManager::new_local(config.storage.root.clone());
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path: Arc::new(config_path.into()),
            database: Arc::new(OnceLock::new()),
            cache: Arc::new(OnceLock::new()),
            log_hub: LogStreamHub::new(),
            node_manager,
            storage,
            setup_mutex: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    pub fn try_database(&self) -> Option<&DatabaseConnection> {
        self.database.get()
    }

    pub fn try_cache(&self) -> Option<&grass_cache::CacheStore> {
        self.cache.get()
    }

    pub async fn lock_setup(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.setup_mutex.lock().await
    }
}
