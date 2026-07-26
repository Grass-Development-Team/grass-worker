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

/// One counter to check and pre-consume inside an atomic multi-key quota
/// operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaCounterCheck {
    pub key: String,
    /// Amount to add to the counter.
    pub amount: i64,
    /// Inclusive maximum. A negative maximum means unlimited.
    pub max: i64,
    /// Optional expiry applied when the counter is created or refreshed,
    /// used for periodic counters such as monthly windows.
    pub ttl: Option<Duration>,
}

/// Outcome of an atomic multi-counter check-and-consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaCheckOutcome {
    /// Every counter fit under its maximum and all increments were applied.
    Allowed,
    /// The counter with this key would exceed its maximum. No increments
    /// were left applied.
    Denied { key: String },
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
    /// Stores the value only when the key does not exist yet. Returns whether
    /// the value was stored. Used to seed quota counters from durable state
    /// before atomic increments run against them.
    async fn set_if_absent(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<bool>;
    /// Atomically checks every counter against its maximum and applies all
    /// increments, rolling back already-applied increments when any counter
    /// would exceed its maximum.
    async fn check_and_consume(
        &self,
        checks: &[QuotaCounterCheck],
    ) -> anyhow::Result<QuotaCheckOutcome>;
    /// Adds `amount` (may be negative) to a counter without a limit check,
    /// clamping the result at zero. Used to roll back reserved quota after a
    /// failed business operation.
    async fn adjust_counter(&self, key: &str, amount: i64) -> anyhow::Result<i64>;
    /// Acquires one slot of a bounded semaphore. Returns whether a slot was
    /// acquired. The key expires after `ttl` so crashed holders cannot occupy
    /// slots forever; callers should re-acquire or refresh long-lived slots.
    async fn acquire_slot(&self, key: &str, max: i64, ttl: Duration) -> anyhow::Result<bool>;
    /// Releases one slot of a bounded semaphore, clamping at zero.
    async fn release_slot(&self, key: &str) -> anyhow::Result<()>;
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

    async fn set_if_absent(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<bool> {
        match self {
            Self::Moka(c) => c.set_if_absent(key, value, ttl).await,
            Self::Redis(c) => c.set_if_absent(key, value, ttl).await,
        }
    }

    async fn check_and_consume(
        &self,
        checks: &[QuotaCounterCheck],
    ) -> anyhow::Result<QuotaCheckOutcome> {
        match self {
            Self::Moka(c) => c.check_and_consume(checks).await,
            Self::Redis(c) => c.check_and_consume(checks).await,
        }
    }

    async fn adjust_counter(&self, key: &str, amount: i64) -> anyhow::Result<i64> {
        match self {
            Self::Moka(c) => c.adjust_counter(key, amount).await,
            Self::Redis(c) => c.adjust_counter(key, amount).await,
        }
    }

    async fn acquire_slot(&self, key: &str, max: i64, ttl: Duration) -> anyhow::Result<bool> {
        match self {
            Self::Moka(c) => c.acquire_slot(key, max, ttl).await,
            Self::Redis(c) => c.acquire_slot(key, max, ttl).await,
        }
    }

    async fn release_slot(&self, key: &str) -> anyhow::Result<()> {
        match self {
            Self::Moka(c) => c.release_slot(key).await,
            Self::Redis(c) => c.release_slot(key).await,
        }
    }
}
