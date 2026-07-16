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
    let refreshed = cache
        .update_if_present(&key, &json, Duration::from_secs(remaining as u64))
        .await
        .context("failed to refresh session")?;
    if !refreshed {
        return Ok(None);
    }

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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use grass_cache::{Cache, MokaCache};

    use super::*;

    #[tokio::test]
    async fn session_can_be_created_validated_and_revoked() {
        let cache = MokaCache::connect();
        let session_id = create_session(&cache, Uuid::now_v7(), Duration::from_secs(60))
            .await
            .unwrap();

        assert!(
            validate_session(
                &cache,
                &session_id,
                Duration::from_secs(30),
                Duration::from_secs(60),
            )
            .await
            .unwrap()
            .is_some()
        );
        revoke_session(&cache, &session_id).await.unwrap();
        assert!(
            validate_session(
                &cache,
                &session_id,
                Duration::from_secs(30),
                Duration::from_secs(60),
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn refresh_does_not_recreate_a_concurrently_revoked_session() {
        let cache = DeleteAfterReadCache::default();
        let session_id = create_session(&cache, Uuid::now_v7(), Duration::from_secs(60))
            .await
            .unwrap();

        let session = validate_session(
            &cache,
            &session_id,
            Duration::from_secs(30),
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        assert!(session.is_none());
        assert!(cache.value.lock().unwrap().is_none());
    }

    #[derive(Clone, Default)]
    struct DeleteAfterReadCache {
        value: Arc<Mutex<Option<String>>>,
    }

    #[async_trait::async_trait]
    impl Cache for DeleteAfterReadCache {
        async fn get(&self, _key: &str) -> anyhow::Result<Option<String>> {
            Ok(self.value.lock().unwrap().take())
        }

        async fn set(&self, _key: &str, value: &str, _ttl: Duration) -> anyhow::Result<()> {
            *self.value.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }

        async fn update_if_present(
            &self,
            _key: &str,
            value: &str,
            _ttl: Duration,
        ) -> anyhow::Result<bool> {
            let mut stored = self.value.lock().unwrap();
            if stored.is_none() {
                return Ok(false);
            }
            *stored = Some(value.to_owned());
            Ok(true)
        }

        async fn delete(&self, _key: &str) -> anyhow::Result<()> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }

        async fn incr(&self, _key: &str) -> anyhow::Result<i64> {
            unreachable!()
        }

        async fn decr(&self, _key: &str) -> anyhow::Result<i64> {
            unreachable!()
        }

        async fn consume_rate_limit(
            &self,
            _key: &str,
            _capacity: u32,
            _refill_period: Duration,
        ) -> anyhow::Result<bool> {
            unreachable!()
        }
    }
}
