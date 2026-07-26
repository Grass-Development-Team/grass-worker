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

    async fn set_if_absent(&self, key: &str, value: &str, ttl: Duration) -> anyhow::Result<bool> {
        let mut conn = self.conn.clone();
        let stored: bool = redis::cmd("SET")
            .arg(key)
            .arg(value)
            .arg("NX")
            .arg("PX")
            .arg(ttl.as_millis().max(1) as u64)
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow::anyhow!("redis set-if-absent failed: {e}"))?;
        Ok(stored)
    }

    async fn check_and_consume(
        &self,
        checks: &[super::QuotaCounterCheck],
    ) -> anyhow::Result<super::QuotaCheckOutcome> {
        if checks.is_empty() {
            return Ok(super::QuotaCheckOutcome::Allowed);
        }

        let mut conn = self.conn.clone();
        let script = redis::Script::new(
            r#"
local count = tonumber(ARGV[1])
for i = 1, count do
    local amount = tonumber(ARGV[1 + i])
    local max = tonumber(ARGV[1 + count + i])
    local current = tonumber(redis.call('GET', KEYS[i]) or '0')
    if max >= 0 and current + amount > max then
        return {0, KEYS[i]}
    end
end
for i = 1, count do
    local amount = tonumber(ARGV[1 + i])
    local ttl_ms = tonumber(ARGV[1 + 2 * count + i])
    redis.call('INCRBY', KEYS[i], amount)
    if ttl_ms > 0 and redis.call('PTTL', KEYS[i]) < 0 then
        redis.call('PEXPIRE', KEYS[i], ttl_ms)
    end
end
return {1}
"#,
        );
        let mut invocation = script.prepare_invoke();
        invocation.arg(checks.len());
        for check in checks {
            invocation.key(&check.key);
        }
        for check in checks {
            invocation.arg(check.amount);
        }
        for check in checks {
            invocation.arg(check.max);
        }
        for check in checks {
            invocation.arg(check.ttl.map(|ttl| ttl.as_millis() as u64).unwrap_or(0));
        }

        let result: Vec<redis::Value> = invocation
            .invoke_async(&mut conn)
            .await
            .map_err(|e| anyhow::anyhow!("redis quota check failed: {e}"))?;

        match result.first() {
            Some(redis::Value::Int(1)) => Ok(super::QuotaCheckOutcome::Allowed),
            Some(redis::Value::Int(0)) => {
                let key = match result.get(1) {
                    Some(redis::Value::BulkString(bytes)) => {
                        String::from_utf8_lossy(bytes).into_owned()
                    }
                    Some(redis::Value::SimpleString(text)) => text.clone(),
                    _ => String::new(),
                };
                Ok(super::QuotaCheckOutcome::Denied { key })
            }
            _ => Err(anyhow::anyhow!("redis quota check returned invalid reply")),
        }
    }

    async fn adjust_counter(&self, key: &str, amount: i64) -> anyhow::Result<i64> {
        let mut conn = self.conn.clone();
        let value: i64 = redis::Script::new(
            r#"
local next = tonumber(redis.call('GET', KEYS[1]) or '0') + tonumber(ARGV[1])
if next < 0 then
    next = 0
end
local ttl = redis.call('PTTL', KEYS[1])
redis.call('SET', KEYS[1], next)
if ttl > 0 then
    redis.call('PEXPIRE', KEYS[1], ttl)
end
return next
"#,
        )
        .key(key)
        .arg(amount)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis counter adjust failed: {e}"))?;
        Ok(value)
    }

    async fn acquire_slot(&self, key: &str, max: i64, ttl: Duration) -> anyhow::Result<bool> {
        let mut conn = self.conn.clone();
        let acquired: i64 = redis::Script::new(
            r#"
local max = tonumber(ARGV[1])
local current = tonumber(redis.call('GET', KEYS[1]) or '0')
if max >= 0 and current + 1 > max then
    return 0
end
redis.call('INCR', KEYS[1])
redis.call('PEXPIRE', KEYS[1], tonumber(ARGV[2]))
return 1
"#,
        )
        .key(key)
        .arg(max)
        .arg(ttl.as_millis().max(1) as u64)
        .invoke_async(&mut conn)
        .await
        .map_err(|e| anyhow::anyhow!("redis slot acquire failed: {e}"))?;
        Ok(acquired == 1)
    }

    async fn release_slot(&self, key: &str) -> anyhow::Result<()> {
        self.adjust_counter(key, -1).await.map(|_| ())
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
