use std::net::SocketAddr;

use anyhow::Context;
use grass_cache::{CacheBackend, CacheStore};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tracing::info;

use crate::{
    infra::{config::ControlApiConfig, database, database::entity::system_setting},
    state::ControlApiState,
};

pub async fn cache(state: &ControlApiState) -> anyhow::Result<()> {
    let (backend, redis_url) = {
        let config = state.config.read().unwrap();
        (config.redis.backend, config.redis.url.clone())
    };

    let store = match backend {
        CacheBackend::Redis if redis_url.trim().is_empty() => {
            anyhow::bail!("Redis URL is required when the Redis cache backend is selected")
        }
        CacheBackend::Redis => CacheStore::connect_cache(CacheBackend::Redis, &redis_url).await,
        CacheBackend::Moka => CacheStore::connect_cache(CacheBackend::Moka, "").await,
    }?;

    state.cache.set(store).map_err(|_| {
        anyhow::anyhow!("cache store was already initialized before startup completed")
    })?;
    Ok(())
}

pub fn config(path: &str) -> anyhow::Result<ControlApiConfig> {
    ControlApiConfig::load(path)
        .with_context(|| format!("failed to load Control API config from {path}"))
}

pub async fn database(state: &ControlApiState) -> anyhow::Result<()> {
    let (db_url, auto_migrate) = {
        let config = state.config.read().unwrap();
        (config.database.url.clone(), config.migration.auto_migrate)
    };

    if db_url.trim().is_empty() {
        info!(
            operation = "control_api.database_not_configured",
            "database URL is not configured; entering setup mode"
        );
        return Ok(());
    }

    let db = database::connect(&db_url).await?;
    if auto_migrate {
        migrate_and_seed(&db).await?;
    }

    state.database.set(db).map_err(|_| {
        anyhow::anyhow!("database connection was already initialized before startup completed")
    })?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn configured_database_connection_failure_is_fatal() {
        let mut config = ControlApiConfig::default();
        config.database.url = "postgres://audit:audit@127.0.0.1:1/audit".to_owned();
        let state = ControlApiState::new(config, "unused.toml");

        assert!(database(&state).await.is_err());
        assert!(state.try_database().is_none());
    }

    #[tokio::test]
    async fn configured_redis_connection_failure_is_fatal() {
        let mut config = ControlApiConfig::default();
        config.redis.url = "redis://127.0.0.1:1/0".to_owned();
        let state = ControlApiState::new(config, "unused.toml");

        assert!(cache(&state).await.is_err());
        assert!(state.try_cache().is_none());
    }

    #[tokio::test]
    async fn explicit_moka_backend_initializes_without_redis() {
        let mut config = ControlApiConfig::default();
        config.redis.backend = CacheBackend::Moka;
        let state = ControlApiState::new(config, "unused.toml");

        cache(&state).await.unwrap();
        assert!(state.try_cache().is_some());
    }
}
