use std::time::Duration;

use axum::{
    body::Body,
    http::{Method, Request},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{infra::error::AppError, state::ControlApiState};

const CSRF_KEY_PREFIX: &str = "csrf";
const CSRF_TOKEN_HEADER: &str = "x-csrf-token";
const CSRF_TTL: Duration = Duration::from_secs(2_592_000);

pub fn csrf_key(session_id: &str) -> String {
    format!("{CSRF_KEY_PREFIX}:{session_id}")
}

pub async fn generate_csrf_token(
    cache: &impl grass_cache::Cache,
    session_id: &str,
) -> anyhow::Result<String> {
    let token = grass_token::generate_token();
    let key = csrf_key(session_id);
    cache.set(&key, &token, CSRF_TTL).await?;
    Ok(token)
}

pub async fn validate_csrf_token(
    cache: &impl grass_cache::Cache,
    session_id: &str,
    token: &str,
) -> anyhow::Result<bool> {
    let key = csrf_key(session_id);
    let stored = cache.get(&key).await?;

    match stored {
        Some(stored_token) => {
            let valid =
                subtle::ConstantTimeEq::ct_eq(stored_token.as_bytes(), token.as_bytes()).into();
            Ok(valid)
        }
        None => Ok(false),
    }
}

pub async fn csrf_middleware(
    state: axum::extract::State<ControlApiState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    if !requires_csrf(&method) {
        return next.run(request).await;
    }

    let path = request.uri().path().to_owned();
    if csrf_exempt_path(&path) {
        return next.run(request).await;
    }

    let csrf_token = request
        .headers()
        .get(CSRF_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    let session_id = request
        .extensions()
        .get::<Option<(String, grass_session::SessionData)>>()
        .and_then(|o| o.as_ref().map(|(sid, _)| sid.clone()));

    if session_id.is_none() {
        return next.run(request).await;
    }

    match (csrf_token, session_id, state.try_cache()) {
        (Some(token), Some(sid), Some(cache)) => {
            match validate_csrf_token(cache, &sid, &token).await {
                Ok(true) => next.run(request).await,
                Ok(false) => AppError::Forbidden {
                    op: "csrf.invalid_token",
                    message: "invalid or missing CSRF token".to_owned(),
                }
                .into_response(),
                Err(source) => AppError::Infrastructure {
                    op: "csrf.validate",
                    source,
                }
                .into_response(),
            }
        }
        _ => AppError::Forbidden {
            op: "csrf.missing_token",
            message: "CSRF token required for mutation requests".to_owned(),
        }
        .into_response(),
    }
}

fn csrf_exempt_path(path: &str) -> bool {
    matches!(path, "/auth/login" | "/auth/register") || path.starts_with("/setup/")
}

fn requires_csrf(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

#[cfg(test)]
mod tests {
    use axum::{Router, middleware, routing::post};
    use grass_cache::{CacheBackend, CacheStore};
    use std::time::Duration;
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::{
        features::api::v1::auth::logout,
        infra::{
            config::ControlApiConfig,
            http::{
                extractors::Session,
                middlewares::{csrf, session},
            },
        },
        state::ControlApiState,
    };

    use super::*;

    #[test]
    fn only_pre_auth_and_setup_paths_are_exempt() {
        assert!(csrf_exempt_path("/auth/login"));
        assert!(csrf_exempt_path("/auth/register"));
        assert!(csrf_exempt_path("/setup/database"));
        assert!(!csrf_exempt_path("/auth/logout"));
        assert!(!csrf_exempt_path("/teams"));
    }

    async fn authenticated_app() -> (Router, CacheStore, String) {
        let cache = CacheStore::connect_cache(CacheBackend::Moka, "")
            .await
            .unwrap();
        let session_id =
            grass_session::create_session(&cache, Uuid::now_v7(), Duration::from_secs(300))
                .await
                .unwrap();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.cache.set(cache.clone()).is_ok());

        async fn protected(_session: Session) -> &'static str {
            "ok"
        }

        let app = Router::new()
            .route("/teams", post(protected))
            .route("/auth/logout", post(logout::handler))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                csrf::csrf_middleware,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                session::session_middleware,
            ))
            .with_state(state);

        (app, cache, session_id)
    }

    #[tokio::test]
    async fn unauthenticated_mutation_reaches_session_guard() {
        let (app, _, _) = authenticated_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/teams")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authenticated_logout_requires_csrf_token() {
        let (app, _, session_id) = authenticated_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/logout")
                    .header("cookie", format!("session_id={session_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn csrf_protected_logout_revokes_session() {
        let (app, cache, session_id) = authenticated_app().await;
        let token = generate_csrf_token(&cache, &session_id).await.unwrap();
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/auth/logout")
                    .header("cookie", format!("session_id={session_id}"))
                    .header(CSRF_TOKEN_HEADER, token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert!(
            grass_session::validate_session(
                &cache,
                &session_id,
                Duration::from_secs(60),
                Duration::from_secs(300),
            )
            .await
            .unwrap()
            .is_none()
        );
    }
}
