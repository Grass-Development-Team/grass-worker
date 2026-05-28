use std::time::Duration;

use anyhow::Context;
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

fn session_key(session_id: &str) -> String {
    format!("{SESSION_KEY_PREFIX}:{session_id}")
}

pub async fn create_session(
    cache: &impl grass_cache::Cache,
    user_id: Uuid,
    absolute_ttl: Duration,
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
    cache
        .set(&key, &json, absolute_ttl)
        .await
        .context("failed to store session")?;
    Ok(session_id)
}

pub async fn validate_session(
    cache: &impl grass_cache::Cache,
    session_id: &str,
    idle_ttl: Duration,
    absolute_ttl: Duration,
) -> anyhow::Result<Option<SessionData>> {
    let key = session_key(session_id);
    let raw = cache.get(&key).await.context("failed to read session")?;

    let raw = match raw {
        Some(r) => r,
        None => return Ok(None),
    };

    let mut data: SessionData =
        serde_json::from_str(&raw).context("failed to deserialize session data")?;

    let now = OffsetDateTime::now_utc();

    let idle_duration = now - data.last_accessed_at;
    if idle_duration.whole_seconds() > idle_ttl.as_secs() as i64 {
        cache
            .delete(&key)
            .await
            .context("failed to delete expired session")?;
        return Ok(None);
    }

    let remaining = absolute_ttl.as_secs() as i64 - (now - data.created_at).whole_seconds();
    if remaining <= 0 {
        cache
            .delete(&key)
            .await
            .context("failed to delete expired session")?;
        return Ok(None);
    }

    data.last_accessed_at = now;
    let json = serde_json::to_string(&data).context("failed to serialize refreshed session")?;
    cache
        .set(&key, &json, Duration::from_secs(remaining as u64))
        .await
        .context("failed to refresh session")?;

    Ok(Some(data))
}

pub async fn revoke_session(
    cache: &impl grass_cache::Cache,
    session_id: &str,
) -> anyhow::Result<()> {
    let key = session_key(session_id);
    cache
        .delete(&key)
        .await
        .context("failed to revoke session")?;
    Ok(())
}
