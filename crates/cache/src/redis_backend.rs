use std::time::Duration;

use redis::aio::MultiplexedConnection;

#[derive(Clone)]
pub struct RedisCache {
    conn: MultiplexedConnection,
}

impl RedisCache {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client =
            redis::Client::open(url).map_err(|e| anyhow::anyhow!("invalid Redis URL: {e}"))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| anyhow::anyhow!("Redis connection failed: {e}"))?;
        Ok(Self { conn })
    }
}

#[async_trait::async_trait]
impl super::Cache for RedisCache {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn.clone();
        let value: Option<String> = redis::AsyncCommands::get(&mut conn, key)
            .await
            .map_err(|e| anyhow::anyhow!("redis get failed: {e}"))?;
        Ok(value)
    }

    async fn set(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: () = redis::AsyncCommands::set_ex(&mut conn, key, value, ttl.as_secs())
            .await
            .map_err(|e| anyhow::anyhow!("redis set failed: {e}"))?;
        Ok(())
    }

    async fn update_if_present(
        &self,
        key: &str,
        value: &str,
        ttl: Duration,
    ) -> anyhow::Result<bool> {
        let mut conn = self.conn.clone();
        let updated: i64 = redis::Script::new(
            r#"
if redis.call('EXISTS', KEYS[1]) == 0 then
    return 0
end
redis.call('SET', KEYS[1], ARGV[1], 'PX', ARGV[2])
return 1
"#,
        )
        .key(key)
        .arg(value)
        .arg(ttl.as_millis().max(1) as u64)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis conditional update failed: {e}"))?;
        Ok(updated == 1)
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let _: () = redis::AsyncCommands::del(&mut conn, key)
            .await
            .map_err(|e| anyhow::anyhow!("redis delete failed: {e}"))?;
        Ok(())
    }

    async fn incr(&self, key: &str) -> anyhow::Result<i64> {
        let mut conn = self.conn.clone();
        let value: i64 = redis::AsyncCommands::incr(&mut conn, key, 1)
            .await
            .map_err(|e| anyhow::anyhow!("redis incr failed: {e}"))?;
        Ok(value)
    }

    async fn decr(&self, key: &str) -> anyhow::Result<i64> {
        let mut conn = self.conn.clone();
        let value: i64 = redis::AsyncCommands::decr(&mut conn, key, 1)
            .await
            .map_err(|e| anyhow::anyhow!("redis decr failed: {e}"))?;
        Ok(value)
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

        let mut conn = self.conn.clone();
        let allowed: i64 = redis::Script::new(
            r#"
local capacity = tonumber(ARGV[1])
local period_ms = tonumber(ARGV[2])
local now = redis.call('TIME')
local now_ms = tonumber(now[1]) * 1000 + math.floor(tonumber(now[2]) / 1000)
local bucket = redis.call('HMGET', KEYS[1], 'tokens', 'updated_at')
local tokens = tonumber(bucket[1]) or capacity
local updated_at = tonumber(bucket[2]) or now_ms
local refill_rate = capacity / period_ms
tokens = math.min(capacity, tokens + math.max(0, now_ms - updated_at) * refill_rate)
local allowed = 0
if tokens >= 1 then
    tokens = tokens - 1
    allowed = 1
end
redis.call('HSET', KEYS[1], 'tokens', tokens, 'updated_at', now_ms)
redis.call('PEXPIRE', KEYS[1], period_ms * 2)
return allowed
"#,
        )
        .key(key)
        .arg(capacity)
        .arg(refill_period.as_millis().max(1) as u64)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis rate limit failed: {e}"))?;
        Ok(allowed == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Cache;
    use super::*;

    async fn test_cache() -> Option<RedisCache> {
        let url = std::env::var("GRASS_TEST_REDIS_URL").ok()?;
        Some(RedisCache::connect(&url).await.unwrap())
    }

    fn unique_key(suffix: &str) -> String {
        format!(
            "grass:test:{}:{}:{suffix}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        )
    }

    #[tokio::test]
    async fn conditional_update_does_not_recreate_deleted_value() {
        let Some(cache) = test_cache().await else {
            return;
        };
        let key = unique_key("conditional");
        cache
            .set(&key, "old", Duration::from_secs(60))
            .await
            .unwrap();
        cache.delete(&key).await.unwrap();

        assert!(
            !cache
                .update_if_present(&key, "new", Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert_eq!(cache.get(&key).await.unwrap(), None);
    }

    #[tokio::test]
    async fn token_bucket_is_atomic_in_redis() {
        let Some(cache) = test_cache().await else {
            return;
        };
        let key = unique_key("rate-limit");

        assert!(
            cache
                .consume_rate_limit(&key, 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            cache
                .consume_rate_limit(&key, 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
        assert!(
            !cache
                .consume_rate_limit(&key, 2, Duration::from_secs(60))
                .await
                .unwrap()
        );
        cache.delete(&key).await.unwrap();
    }
}
