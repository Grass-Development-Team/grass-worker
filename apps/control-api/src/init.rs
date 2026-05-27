use std::net::SocketAddr;

use anyhow::Context;
use grass_cache::{CacheBackend, CacheStore, MokaCache, RedisCache, redis_backend};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tracing::info;

use crate::{
    infra::{config::ControlApiConfig, database, database::entity::system_setting},
    state::ControlApiState,
};

pub async fn cache(state: &ControlApiState) {
    let (backend, redis_url) = {
        let config = state.config.read().unwrap();
        (config.cache.backend, config.cache.redis_url.clone())
    };

    let store = match backend {
        CacheBackend::Redis => {
            if redis_url.trim().is_empty() {
                tracing::warn!(
                    operation = "control_api.cache.redis_url_empty",
                    "Redis URL is empty; falling back to moka"
                );
                CacheStore::Moka(MokaCache::new(10_000))
            } else {
                match redis_backend::connect(&redis_url).await {
                    Ok(conn) => CacheStore::Redis(RedisCache::new(conn)),
                    Err(error) => {
                        tracing::warn!(
                            operation = "control_api.cache.redis_failed",
                            %error,
                            "Redis unavailable; falling back to moka"
                        );
                        CacheStore::Moka(MokaCache::new(10_000))
                    }
                }
            }
        }
        CacheBackend::Moka => CacheStore::Moka(MokaCache::new(10_000)),
    };

    state.cache.set(store).ok();
}

pub fn config(path: &str) -> anyhow::Result<ControlApiConfig> {
    ControlApiConfig::load(path)
        .with_context(|| format!("failed to load Control API config from {path}"))
}

pub async fn database(state: &ControlApiState) -> anyhow::Result<bool> {
    let (db_url, auto_migrate) = {
        let config = state.config.read().unwrap();
        (config.database.url.clone(), config.migration.auto_migrate)
    };

    if db_url.trim().is_empty() {
        info!(
            operation = "control_api.database_not_configured",
            "database URL is not configured; entering setup mode"
        );
        return Ok(true);
    }

    match database::connect(&db_url).await {
        Ok(db) => {
            if auto_migrate {
                migrate_and_seed(&db).await?;
            }

            let is_setup_mode = !is_setup_finished(&db).await.unwrap_or(false);
            state.database.set(db).ok();
            Ok(is_setup_mode)
        }
        Err(error) => {
            tracing::warn!(operation = "control_api.db_failed", %error, "database unavailable; entering setup mode");
            Ok(true)
        }
    }
}

pub async fn migrate(state: &ControlApiState) -> anyhow::Result<()> {
    let db_url = state.config.read().unwrap().database.url.clone();
    let db = database::connect(&db_url).await?;
    migrate_and_seed(&db).await?;
    info!(
        operation = "control_api.migrate",
        "database migrations and seed completed"
    );
    Ok(())
}

pub fn address(state: &ControlApiState) -> SocketAddr {
    let config = state.config.read().unwrap();
    SocketAddr::new(config.server.host, config.server.port)
}

pub async fn migrate_and_seed(db: &DatabaseConnection) -> anyhow::Result<()> {
    database::migrate::run(db).await?;
    database::seed::run(db).await?;
    Ok(())
}

pub async fn is_setup_finished(db: &DatabaseConnection) -> anyhow::Result<bool> {
    let setting = system_setting::Entity::find()
        .filter(system_setting::Column::Key.eq("setup.finished"))
        .one(db)
        .await?;

    Ok(setting.and_then(|s| s.value.as_bool()).unwrap_or(false))
}
