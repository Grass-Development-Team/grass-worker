pub mod moka_backend;
pub mod redis_backend;

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use moka_backend::MokaCache;
pub use redis_backend::RedisCache;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheBackend {
    #[default]
    Moka,
    Redis,
}

impl std::fmt::Display for CacheBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Moka => write!(f, "moka"),
            Self::Redis => write!(f, "redis"),
        }
    }
}

#[async_trait::async_trait]
pub trait Cache: Send + Sync + Clone + 'static {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>>;
    async fn set(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<()>;
    async fn update_if_present(
        &self,
        key: &str,
        value: &str,
        ttl: Duration,
    ) -> anyhow::Result<bool>;
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
    async fn incr(&self, key: &str) -> anyhow::Result<i64>;
    async fn decr(&self, key: &str) -> anyhow::Result<i64>;
    async fn consume_rate_limit(
        &self,
        key: &str,
        capacity: u32,
        refill_period: Duration,
    ) -> anyhow::Result<bool>;
}

#[derive(Clone)]
pub enum CacheStore {
    Moka(MokaCache),
    Redis(RedisCache),
}

impl CacheStore {
    pub async fn connect_cache(backend: CacheBackend, url: &str) -> anyhow::Result<Self> {
        match backend {
            CacheBackend::Moka => Ok(Self::Moka(MokaCache::connect())),
            CacheBackend::Redis => Ok(Self::Redis(RedisCache::connect(url).await?)),
        }
    }
}

#[async_trait::async_trait]
impl Cache for CacheStore {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        match self {
            Self::Moka(c) => c.get(key).await,
            Self::Redis(c) => c.get(key).await,
        }
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<()> {
        match self {
            Self::Moka(c) => c.set(key, value, ttl).await,
            Self::Redis(c) => c.set(key, value, ttl).await,
        }
    }

    async fn update_if_present(
        &self,
        key: &str,
        value: &str,
        ttl: Duration,
    ) -> anyhow::Result<bool> {
        match self {
            Self::Moka(c) => c.update_if_present(key, value, ttl).await,
            Self::Redis(c) => c.update_if_present(key, value, ttl).await,
        }
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        match self {
            Self::Moka(c) => c.delete(key).await,
            Self::Redis(c) => c.delete(key).await,
        }
    }

    async fn incr(&self, key: &str) -> anyhow::Result<i64> {
        match self {
            Self::Moka(c) => c.incr(key).await,
            Self::Redis(c) => c.incr(key).await,
        }
    }

    async fn decr(&self, key: &str) -> anyhow::Result<i64> {
        match self {
            Self::Moka(c) => c.decr(key).await,
            Self::Redis(c) => c.decr(key).await,
        }
    }

    async fn consume_rate_limit(
        &self,
        key: &str,
        capacity: u32,
        refill_period: Duration,
    ) -> anyhow::Result<bool> {
        match self {
            Self::Moka(c) => c.consume_rate_limit(key, capacity, refill_period).await,
            Self::Redis(c) => c.consume_rate_limit(key, capacity, refill_period).await,
        }
    }
}
