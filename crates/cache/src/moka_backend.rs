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
    incr_mutex: Arc<Mutex<()>>,
}

impl MokaCache {
    pub fn new(max_capacity: u64) -> Self {
        let inner = CacheBuilder::new(max_capacity)
            .expire_after(MokaExpiry)
            .build();
        Self {
            inner,
            incr_mutex: Arc::new(Mutex::new(())),
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

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.inner.invalidate(key).await;
        Ok(())
    }

    async fn incr(&self, key: &str) -> anyhow::Result<i64> {
        let _guard = self.incr_mutex.lock().await;

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
        let _guard = self.incr_mutex.lock().await;

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
}
