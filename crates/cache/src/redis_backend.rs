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
}
