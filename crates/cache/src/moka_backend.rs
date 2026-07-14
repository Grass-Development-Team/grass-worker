use std::sync::Arc;
use std::time::Duration;

use moka::Expiry;
use moka::future::{Cache as MokaInner, CacheBuilder};
use tokio::sync::Mutex;

fn expiry_for_value(value: &str) -> Option<Duration> {
    extract_value(value).map(|(expiry, _)| {
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let remaining = (expiry - now).max(0) as u64;
        Duration::from_secs(remaining)
    })
}

struct MokaExpiry;

impl Expiry<String, String> for MokaExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &String,
        _current_at: std::time::Instant,
    ) -> Option<Duration> {
        expiry_for_value(value)
    }

    fn expire_after_read(
        &self,
        _key: &String,
        _value: &String,
        _read_at: std::time::Instant,
        duration_until_expiry: Option<Duration>,
        _last_modified_at: std::time::Instant,
    ) -> Option<Duration> {
        duration_until_expiry
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &String,
        _updated_at: std::time::Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        expiry_for_value(value)
    }
}

#[derive(Clone)]
pub struct MokaCache {
    inner: MokaInner<String, String>,
    atomic_mutex: Arc<Mutex<()>>,
}

impl MokaCache {
    pub fn new(max_capacity: u64) -> Self {
        let inner = CacheBuilder::new(max_capacity)
            .expire_after(MokaExpiry)
            .build();
        Self {
            inner,
            atomic_mutex: Arc::new(Mutex::new(())),
        }
    }

    pub fn connect() -> Self {
        Self::new(10_000)
    }
}

fn extract_value(raw: &str) -> Option<(i64, &str)> {
    raw.split_once(':')
        .and_then(|(ts, val)| ts.parse::<i64>().ok().map(|expiry| (expiry, val)))
}

#[async_trait::async_trait]
impl super::Cache for MokaCache {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let raw = self.inner.get(key).await;
        Ok(raw.map(|v| {
            extract_value(&v)
                .map(|(_, val)| val.to_owned())
                .unwrap_or(v)
        }))
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<()> {
        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64;
        let stored = format!("{expiry}:{value}");
        self.inner.insert(key.to_owned(), stored).await;
        Ok(())
    }

    async fn update_if_present(
        &self,
        key: &str,
        value: &str,
        ttl: Duration,
    ) -> anyhow::Result<bool> {
        let _guard = self.atomic_mutex.lock().await;
        if self.inner.get(key).await.is_none() {
            return Ok(false);
        }

        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64;
        self.inner
            .insert(key.to_owned(), format!("{expiry}:{value}"))
            .await;
        Ok(true)
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let _guard = self.atomic_mutex.lock().await;
        self.inner.invalidate(key).await;
        Ok(())
    }

    async fn incr(&self, key: &str) -> anyhow::Result<i64> {
        let _guard = self.atomic_mutex.lock().await;

        let current = self
            .inner
            .get(key)
            .await
            .and_then(|v| {
                let num = extract_value(&v).map(|(_, val)| val).unwrap_or(&v);
                num.parse::<i64>().ok()
            })
            .unwrap_or(0);
        let next = current + 1;
        self.inner.insert(key.to_owned(), next.to_string()).await;
        Ok(next)
    }

    async fn decr(&self, key: &str) -> anyhow::Result<i64> {
        let _guard = self.atomic_mutex.lock().await;

        let current = self
            .inner
            .get(key)
            .await
            .and_then(|v| {
                let num = extract_value(&v).map(|(_, val)| val).unwrap_or(&v);
                num.parse::<i64>().ok()
            })
            .unwrap_or(0);
        let next = current - 1;
        self.inner.insert(key.to_owned(), next.to_string()).await;
        Ok(next)
    }

    async fn consume_rate_limit(
        &self,
        key: &str,
        capacity: u32,
        refill_period: Duration,
    ) -> anyhow::Result<bool> {
        if capacity == 0 || refill_period.is_zero() {
            anyhow::bail!("rate limit capacity and refill period must be positive");
        }

        let _guard = self.atomic_mutex.lock().await;
        let now_ms = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as f64 / 1_000_000.0;
        let (last_ms, stored_tokens) = self
            .get(key)
            .await?
            .and_then(|value| parse_rate_limit(&value))
            .unwrap_or((now_ms, capacity as f64));
        let refill_rate = capacity as f64 / refill_period.as_millis() as f64;
        let elapsed_ms = (now_ms - last_ms).max(0.0);
        let mut tokens = (stored_tokens + elapsed_ms * refill_rate).min(capacity as f64);
        let allowed = tokens >= 1.0;
        if allowed {
            tokens -= 1.0;
        }

        let ttl = refill_period.checked_mul(2).unwrap_or(refill_period);
        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64;
        self.inner
            .insert(key.to_owned(), format!("{expiry}:{now_ms}|{tokens}"))
            .await;
        Ok(allowed)
    }
}

fn parse_rate_limit(value: &str) -> Option<(f64, f64)> {
    let (last_ms, tokens) = value.split_once('|')?;
    Some((last_ms.parse().ok()?, tokens.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::super::Cache;
    use super::*;

    #[tokio::test]
    async fn update_if_present_does_not_recreate_deleted_value() {
        let cache = MokaCache::connect();
        cache
            .set("session:test", "old", Duration::from_secs(60))
            .await
            .unwrap();
        cache.delete("session:test").await.unwrap();

        assert!(
            !cache
                .update_if_present("session:test", "new", Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert_eq!(cache.get("session:test").await.unwrap(), None);
    }

    #[tokio::test]
    async fn token_bucket_rejects_requests_beyond_capacity() {
        let cache = MokaCache::connect();

        assert!(
            cache
                .consume_rate_limit("login:test", 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            cache
                .consume_rate_limit("login:test", 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            !cache
                .consume_rate_limit("login:test", 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
    }
}
