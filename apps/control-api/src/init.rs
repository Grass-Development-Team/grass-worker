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

pub async fn storage(state: &ControlApiState) -> anyhow::Result<()> {
    let Some(db) = state.try_database() else {
        return Ok(());
    };
    let (legacy_root, platform_secret) = {
        let config = state.config.read().unwrap();
        (
            config.storage.root.clone(),
            config.secrets.secret_key.clone(),
        )
    };
    let loaded =
        if let Some(loaded) = crate::domain::storage_settings::load(db, &platform_secret).await? {
            loaded
        } else if is_setup_finished(db).await?
            || crate::domain::storage_settings::has_legacy_root(db).await?
        {
            crate::domain::storage_settings::seed_local(db, &legacy_root, &platform_secret).await?
        } else {
            return Ok(());
        };
    state.storage.replace(loaded.config, loaded.credentials)?;
    if crate::domain::storage_migrations::has_active(db).await? {
        state.storage.mark_maintenance();
    }
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
    use sea_orm::{DbBackend, MockDatabase};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;

    fn setting(key: &str) -> system_setting::Model {
        system_setting::Model {
            id: Uuid::now_v7(),
            key: key.to_owned(),
            value_kind: crate::infra::database::entity::SystemSettingValueKind::Json,
            value: serde_json::Value::Null,
            is_secret: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

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

    #[tokio::test]
    async fn unfinished_setup_without_legacy_storage_does_not_seed_storage_config() {
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([
                Vec::<system_setting::Model>::new(),
                Vec::<system_setting::Model>::new(),
                Vec::<system_setting::Model>::new(),
                vec![setting(crate::domain::storage_settings::CONFIG_KEY)],
                Vec::<system_setting::Model>::new(),
                vec![setting(crate::domain::storage_settings::CREDENTIALS_KEY)],
                Vec::<system_setting::Model>::new(),
                vec![setting(crate::domain::storage_settings::LEGACY_ROOT_KEY)],
            ])
            .append_query_results([Vec::<
                crate::infra::database::entity::storage_migration_job::Model,
            >::new()])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.database.set(database.clone()).unwrap();

        storage(&state).await.unwrap();

        let statements = format!("{:?}", database.into_transaction_log());
        assert!(!statements.contains("INSERT INTO \\\"system_settings\\\""));
    }
}
