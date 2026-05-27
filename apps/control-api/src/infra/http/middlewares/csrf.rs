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
    if path.starts_with("/api/v1/auth/") || path.starts_with("/api/v1/setup/") {
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

    match (csrf_token, session_id, state.try_cache()) {
        (Some(token), Some(sid), Some(cache)) => {
            match validate_csrf_token(cache, &sid, &token).await {
                Ok(true) => next.run(request).await,
                _ => AppError::Forbidden {
                    op: "csrf.invalid_token",
                    message: "invalid or missing CSRF token".to_owned(),
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

fn requires_csrf(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}
