use std::time::Duration;

use axum::{
    body::Body,
    http::{HeaderMap, Request},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{infra::error::AppError, state::ControlApiState};

const SESSION_COOKIE: &str = "session_id";
pub async fn session_middleware(
    state: axum::extract::State<ControlApiState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let session_id = extract_session_cookie(request.headers());
    match (session_id, state.try_cache()) {
        (Some(sid), Some(_)) => {
            let session_data =
                match validate_current_session(&state, &sid, "auth.session.validate").await {
                    Ok(session_data) => session_data,
                    Err(error) => return error.into_response(),
                };
            request
                .extensions_mut()
                .insert::<Option<(String, grass_session::SessionData)>>(
                    session_data.map(|data| (sid, data)),
                );
        }
        _ => {
            request
                .extensions_mut()
                .insert::<Option<(String, grass_session::SessionData)>>(None);
        }
    }

    next.run(request).await
}

fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookie_str| {
            cookie_str.split(';').find_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                if name == SESSION_COOKIE {
                    Some(value.to_owned())
                } else {
                    None
                }
            })
        })
}

/// Shared by HTTP requests and preview credentials which reference a source session.
/// The database is authoritative: refreshing a cached session cannot restore a revoked version.
pub(crate) async fn validate_current_session(
    state: &ControlApiState,
    session_id: &str,
    op: &'static str,
) -> Result<Option<grass_session::SessionData>, AppError> {
    let cache = crate::infra::http::cache(state, op)?;
    let (idle_ttl, absolute_ttl) = {
        let config = state.config.read().unwrap();
        (
            Duration::from_secs(config.session.idle_ttl_seconds),
            Duration::from_secs(config.session.session_ttl_seconds),
        )
    };
    let Some(session) = grass_session::validate_session(cache, session_id, idle_ttl, absolute_ttl)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    else {
        return Ok(None);
    };
    let user = crate::domain::users::get_user_by_id(
        crate::infra::http::database(state, op)?,
        session.user_id,
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    if user.is_some_and(|user| {
        user.deleted_at.is_none()
            && user.status == crate::infra::database::entity::UserStatus::Active
            && session.auth_version > 0
            && user.auth_version == session.auth_version
    }) {
        return Ok(Some(session));
    }
    grass_session::revoke_session(cache, session_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?;
    Ok(None)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::infra::{
        config::ControlApiConfig,
        database::entity::{PlatformRole, UserStatus, user},
        http::extractors::Session,
    };
    use axum::{Router, middleware, routing::get};
    use grass_cache::{Cache, CacheStore, MokaCache};
    use sea_orm::{DbBackend, MockDatabase};
    use tower::ServiceExt;

    pub(crate) fn active_user() -> user::Model {
        let now = time::OffsetDateTime::now_utc();
        user::Model {
            id: uuid::Uuid::now_v7(),
            email: "session@example.test".into(),
            display_name: None,
            avatar_version: None,
            auth_version: 1,
            status: UserStatus::Active,
            platform_role: PlatformRole::User,
            email_verified_at: Some(now),
            last_login_at: None,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    async fn protected(_session: Session) -> &'static str {
        "allowed"
    }

    async fn request(state: ControlApiState, sid: &str, method: &str) -> axum::http::StatusCode {
        Router::new()
            .route("/protected", get(protected).post(protected))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session_middleware,
            ))
            .with_state(state)
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .method(method)
                    .header("cookie", format!("session_id={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    async fn state_with_user(user: Option<user::Model>, cache: CacheStore) -> ControlApiState {
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.cache.set(cache).ok().unwrap();
        state
            .database
            .set(
                MockDatabase::new(DbBackend::Postgres)
                    .append_query_results([user.into_iter().collect::<Vec<_>>()])
                    .into_connection(),
            )
            .ok()
            .unwrap();
        state
    }

    async fn revocation_requests(cache: CacheStore) {
        let user = active_user();
        for method in ["GET", "POST"] {
            for scenario in [
                "active",
                "disabled",
                "deleted",
                "missing",
                "revoked",
                "reenabled",
                "legacy",
            ] {
                let mut current = user.clone();
                match scenario {
                    "disabled" => current.status = UserStatus::Disabled,
                    "deleted" => current.deleted_at = Some(time::OffsetDateTime::now_utc()),
                    "revoked" | "reenabled" => current.auth_version = 2,
                    _ => {}
                }
                let sid =
                    grass_session::create_session(&cache, user.id, 1, Duration::from_secs(300))
                        .await
                        .unwrap();
                if scenario == "legacy" {
                    let key = format!("session:{sid}");
                    let mut value: serde_json::Value =
                        serde_json::from_str(&cache.get(&key).await.unwrap().unwrap()).unwrap();
                    value.as_object_mut().unwrap().remove("auth_version");
                    cache
                        .set(&key, &value.to_string(), Duration::from_secs(300))
                        .await
                        .unwrap();
                }
                let state =
                    state_with_user((scenario != "missing").then_some(current), cache.clone())
                        .await;
                let expected = if scenario == "active" { 200 } else { 401 };
                assert_eq!(
                    request(state, &sid, method).await.as_u16(),
                    expected,
                    "{scenario} {method}"
                );
                grass_session::revoke_session(&cache, &sid).await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn moka_rejects_revoked_accounts_at_read_and_write_boundaries() {
        revocation_requests(CacheStore::Moka(MokaCache::connect())).await;
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_REDIS_URL"]
    async fn redis_rejects_revoked_accounts_at_read_and_write_boundaries() {
        let url = std::env::var("GRASS_TEST_REDIS_URL").expect("GRASS_TEST_REDIS_URL is required");
        revocation_requests(CacheStore::Redis(
            grass_cache::RedisCache::connect(&url).await.unwrap(),
        ))
        .await;
    }

    #[tokio::test]
    async fn unavailable_database_never_authenticates_a_cached_session() {
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        let cache = CacheStore::Moka(MokaCache::connect());
        let sid =
            grass_session::create_session(&cache, active_user().id, 1, Duration::from_secs(300))
                .await
                .unwrap();
        state.cache.set(cache).ok().unwrap();
        assert_eq!(request(state, &sid, "GET").await.as_u16(), 500);
    }

    #[tokio::test]
    async fn disabling_an_account_invalidates_two_sessions_after_refresh() {
        let user = active_user();
        let mut disabled = user.clone();
        disabled.status = UserStatus::Disabled;
        disabled.auth_version = 2;
        let cache = CacheStore::Moka(MokaCache::connect());
        let first = grass_session::create_session(&cache, user.id, 1, Duration::from_secs(300))
            .await
            .unwrap();
        let second = grass_session::create_session(&cache, user.id, 1, Duration::from_secs(300))
            .await
            .unwrap();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        state.cache.set(cache.clone()).ok().unwrap();
        state
            .database
            .set(
                MockDatabase::new(DbBackend::Postgres)
                    .append_query_results([
                        vec![user.clone()],
                        vec![disabled.clone()],
                        vec![disabled.clone()],
                        vec![disabled],
                    ])
                    .into_connection(),
            )
            .ok()
            .unwrap();
        assert_eq!(request(state.clone(), &first, "GET").await.as_u16(), 200);
        crate::domain::users::update_user(
            state.try_database().unwrap(),
            user,
            crate::domain::users::UpdateUserParams {
                display_name: None,
                status: Some(UserStatus::Disabled),
                platform_role: None,
            },
        )
        .await
        .unwrap();
        // Even a successful concurrent cache refresh retains the old authentication version.
        grass_session::validate_session(
            &cache,
            &second,
            Duration::from_secs(300),
            Duration::from_secs(300),
        )
        .await
        .unwrap();
        assert_eq!(request(state.clone(), &first, "GET").await.as_u16(), 401);
        assert_eq!(request(state, &second, "POST").await.as_u16(), 401);
    }
}
