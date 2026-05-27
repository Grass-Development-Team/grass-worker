use anyhow::Context;
use redis::{AsyncCommands, aio::MultiplexedConnection};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

const SESSION_KEY_PREFIX: &str = "session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub user_id: Uuid,
    pub created_at: OffsetDateTime,
    pub last_accessed_at: OffsetDateTime,
}

pub fn session_key(session_id: &str) -> String {
    format!("{SESSION_KEY_PREFIX}:{session_id}")
}

pub async fn create_session(
    conn: &mut MultiplexedConnection,
    user_id: Uuid,
    absolute_ttl_seconds: u64,
) -> anyhow::Result<String> {
    let session_id = grass_token::generate_token();
    let now = OffsetDateTime::now_utc();
    let data = SessionData {
        user_id,
        created_at: now,
        last_accessed_at: now,
    };
    let json = serde_json::to_string(&data).context("failed to serialize session data")?;
    let key = session_key(&session_id);
    let _: () = conn
        .set_ex(&key, json, absolute_ttl_seconds)
        .await
        .context("failed to store session in Redis")?;
    Ok(session_id)
}

pub async fn validate_session(
    conn: &mut MultiplexedConnection,
    session_id: &str,
    idle_ttl_seconds: u64,
) -> anyhow::Result<Option<SessionData>> {
    let key = session_key(session_id);
    let raw: Option<String> = conn
        .get(&key)
        .await
        .context("failed to read session from Redis")?;

    let raw = match raw {
        Some(r) => r,
        None => return Ok(None),
    };

    let mut data: SessionData =
        serde_json::from_str(&raw).context("failed to deserialize session data")?;

    let now = OffsetDateTime::now_utc();
    let idle_duration = now - data.last_accessed_at;
    if idle_duration.whole_seconds() > idle_ttl_seconds as i64 {
        let _: () = conn
            .del(&key)
            .await
            .context("failed to delete expired session")?;
        return Ok(None);
    }

    data.last_accessed_at = now;
    let json = serde_json::to_string(&data).context("failed to serialize refreshed session")?;
    let ttl: i64 = conn.ttl(&key).await.context("failed to read session TTL")?;
    if ttl > 0 {
        let _: () = conn
            .set_ex(&key, json, ttl as u64)
            .await
            .context("failed to refresh session TTL")?;
    }

    Ok(Some(data))
}

pub async fn revoke_session(
    conn: &mut MultiplexedConnection,
    session_id: &str,
) -> anyhow::Result<()> {
    let key = session_key(session_id);
    let _: () = conn
        .del(&key)
        .await
        .context("failed to revoke session in Redis")?;
    Ok(())
}
