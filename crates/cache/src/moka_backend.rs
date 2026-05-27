use std::time::Duration;

use moka::future::Cache as MokaInner;

#[derive(Clone)]
pub struct MokaCache {
    inner: MokaInner<String, String>,
}

impl MokaCache {
    pub fn new(max_capacity: u64) -> Self {
        let inner = MokaInner::new(max_capacity);
        Self { inner }
    }

    fn expiry_key(key: &str) -> String {
        format!("__expiry:{key}")
    }
}

#[async_trait::async_trait]
impl super::Cache for MokaCache {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let expiry_key = Self::expiry_key(key);

        if let Some(expiry_ts) = self.inner.get(&expiry_key).await {
            let expiry: i64 = expiry_ts
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid expiry timestamp: {e}"))?;
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            if now > expiry {
                self.inner.invalidate(key).await;
                self.inner.invalidate(&expiry_key).await;
                return Ok(None);
            }
        }

        Ok(self.inner.get(key).await)
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<()> {
        let expiry = time::OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64;
        let expiry_key = Self::expiry_key(key);

        self.inner.insert(key.to_owned(), value.to_owned()).await;
        self.inner.insert(expiry_key, expiry.to_string()).await;

        Ok(())
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let expiry_key = Self::expiry_key(key);
        self.inner.invalidate(key).await;
        self.inner.invalidate(&expiry_key).await;
        Ok(())
    }

    async fn incr(&self, key: &str) -> anyhow::Result<i64> {
        let current = self
            .inner
            .get(key)
            .await
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let next = current + 1;
        self.inner.insert(key.to_owned(), next.to_string()).await;
        Ok(next)
    }

    async fn decr(&self, key: &str) -> anyhow::Result<i64> {
        let current = self
            .inner
            .get(key)
            .await
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let next = current - 1;
        self.inner.insert(key.to_owned(), next.to_string()).await;
        Ok(next)
    }
}
