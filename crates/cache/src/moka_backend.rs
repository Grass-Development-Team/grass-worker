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

    /// Reads a numeric counter and its stored expiry timestamp, treating
    /// missing or non-numeric values as zero. Callers must hold
    /// `atomic_mutex`.
    async fn read_counter_entry(&self, key: &str) -> (i64, Option<i64>) {
        match self.inner.get(key).await {
            Some(raw) => match extract_value(&raw) {
                Some((expiry, value)) => (value.parse::<i64>().unwrap_or(0), Some(expiry)),
                None => (raw.parse::<i64>().unwrap_or(0), None),
            },
            None => (0, None),
        }
    }

    async fn read_counter(&self, key: &str) -> i64 {
        self.read_counter_entry(key).await.0
    }

    /// Writes a numeric counter with an optional absolute expiry timestamp.
    /// Callers must hold `atomic_mutex`.
    async fn write_counter(&self, key: &str, value: i64, expiry: Option<i64>) {
        let stored = match expiry {
            Some(expiry) => format!("{expiry}:{value}"),
            None => value.to_string(),
        };
        self.inner.insert(key.to_owned(), stored).await;
    }
}

fn expiry_from_ttl(ttl: Duration) -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64
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

    async fn take(&self, key: &str) -> anyhow::Result<Option<String>> {
        let _guard = self.atomic_mutex.lock().await;
        let raw = self.inner.get(key).await;
        self.inner.invalidate(key).await;
        Ok(raw.map(|value| {
            extract_value(&value)
                .map(|(_, stored)| stored.to_owned())
                .unwrap_or(value)
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

    async fn set_if_absent(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<bool> {
        let _guard = self.atomic_mutex.lock().await;
        if self.inner.get(key).await.is_some() {
            return Ok(false);
        }

        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64;
        self.inner
            .insert(key.to_owned(), format!("{expiry}:{value}"))
            .await;
        Ok(true)
    }

    async fn check_and_consume(
        &self,
        checks: &[super::QuotaCounterCheck],
    ) -> anyhow::Result<super::QuotaCheckOutcome> {
        let _guard = self.atomic_mutex.lock().await;

        for check in checks {
            let current = self.read_counter(&check.key).await;
            if check.max >= 0 && current + check.amount > check.max {
                return Ok(super::QuotaCheckOutcome::Denied {
                    key: check.key.clone(),
                });
            }
        }

        for check in checks {
            let (current, existing_expiry) = self.read_counter_entry(&check.key).await;
            let expiry = check.ttl.map(expiry_from_ttl).or(existing_expiry);
            self.write_counter(&check.key, current + check.amount, expiry)
                .await;
        }

        Ok(super::QuotaCheckOutcome::Allowed)
    }

    async fn adjust_counter(&self, key: &str, amount: i64) -> anyhow::Result<i64> {
        let _guard = self.atomic_mutex.lock().await;
        let (current, expiry) = self.read_counter_entry(key).await;
        let next = (current + amount).max(0);
        self.write_counter(key, next, expiry).await;
        Ok(next)
    }

    async fn acquire_slot(&self, key: &str, max: i64, ttl: Duration) -> anyhow::Result<bool> {
        let _guard = self.atomic_mutex.lock().await;
        let current = self.read_counter(key).await;
        if max >= 0 && current + 1 > max {
            return Ok(false);
        }
        self.write_counter(key, current + 1, Some(expiry_from_ttl(ttl)))
            .await;
        Ok(true)
    }

    async fn release_slot(&self, key: &str) -> anyhow::Result<()> {
        let _guard = self.atomic_mutex.lock().await;
        let (current, expiry) = self.read_counter_entry(key).await;
        self.write_counter(key, (current - 1).max(0), expiry).await;
        Ok(())
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
    async fn take_returns_a_value_to_only_one_concurrent_caller() {
        let cache = MokaCache::connect();
        cache
            .set("one-time:test", "value", Duration::from_secs(60))
            .await
            .unwrap();

        let first_cache = cache.clone();
        let second_cache = cache.clone();
        let (first, second) = tokio::join!(
            first_cache.take("one-time:test"),
            second_cache.take("one-time:test")
        );
        let values = [first.unwrap(), second.unwrap()];
        assert_eq!(
            values
                .iter()
                .filter(|value| value.as_deref() == Some("value"))
                .count(),
            1
        );
        assert_eq!(values.iter().filter(|value| value.is_none()).count(), 1);
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

    fn check(key: &str, amount: i64, max: i64) -> crate::QuotaCounterCheck {
        crate::QuotaCounterCheck {
            key: key.to_owned(),
            amount,
            max,
            ttl: None,
        }
    }

    #[tokio::test]
    async fn quota_check_consumes_all_dimensions_when_allowed() {
        let cache = MokaCache::connect();
        let outcome = cache
            .check_and_consume(&[check("quota:a", 1, 3), check("quota:b", 2, 5)])
            .await
            .unwrap();

        assert_eq!(outcome, crate::QuotaCheckOutcome::Allowed);
        assert_eq!(cache.get("quota:a").await.unwrap().as_deref(), Some("1"));
        assert_eq!(cache.get("quota:b").await.unwrap().as_deref(), Some("2"));
    }

    #[tokio::test]
    async fn quota_check_denies_and_rolls_back_when_any_dimension_exceeds() {
        let cache = MokaCache::connect();
        cache
            .check_and_consume(&[check("quota:a", 1, 3)])
            .await
            .unwrap();

        let outcome = cache
            .check_and_consume(&[check("quota:a", 1, 3), check("quota:b", 1, 0)])
            .await
            .unwrap();

        assert_eq!(
            outcome,
            crate::QuotaCheckOutcome::Denied {
                key: "quota:b".to_owned()
            }
        );
        assert_eq!(cache.get("quota:a").await.unwrap().as_deref(), Some("1"));
        assert_eq!(cache.get("quota:b").await.unwrap(), None);
    }

    #[tokio::test]
    async fn negative_max_is_unlimited() {
        let cache = MokaCache::connect();
        for _ in 0..10 {
            assert_eq!(
                cache
                    .check_and_consume(&[check("quota:unlimited", 1, -1)])
                    .await
                    .unwrap(),
                crate::QuotaCheckOutcome::Allowed
            );
        }
    }

    #[tokio::test]
    async fn adjust_counter_clamps_at_zero() {
        let cache = MokaCache::connect();
        assert_eq!(cache.adjust_counter("quota:adjust", -5).await.unwrap(), 0);
        assert_eq!(cache.adjust_counter("quota:adjust", 3).await.unwrap(), 3);
        assert_eq!(cache.adjust_counter("quota:adjust", -1).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn slots_are_bounded_and_released() {
        let cache = MokaCache::connect();
        let ttl = Duration::from_secs(60);

        assert!(cache.acquire_slot("slots:test", 2, ttl).await.unwrap());
        assert!(cache.acquire_slot("slots:test", 2, ttl).await.unwrap());
        assert!(!cache.acquire_slot("slots:test", 2, ttl).await.unwrap());

        cache.release_slot("slots:test").await.unwrap();
        assert!(cache.acquire_slot("slots:test", 2, ttl).await.unwrap());
    }

    #[tokio::test]
    async fn set_if_absent_only_stores_once() {
        let cache = MokaCache::connect();
        assert!(
            cache
                .set_if_absent("seed:test", "7", Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            !cache
                .set_if_absent("seed:test", "9", Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert_eq!(cache.get("seed:test").await.unwrap().as_deref(), Some("7"));
    }
}
