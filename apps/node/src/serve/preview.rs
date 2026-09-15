//! Host-scoped preview grants, authorization callbacks and cookie handling.

use std::time::Instant;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use tracing::warn;

use super::{ServeState, response::error_page};
use crate::client::PreviewAuthError;

pub(super) const SECURE_PREVIEW_COOKIE: &str = "__Host-gw_preview_access";
pub(super) const INSECURE_PREVIEW_COOKIE: &str = "gw_preview_access";
pub(super) const PREVIEW_CALLBACK_PATH: &str = "/.grass/auth/callback";

pub(super) fn is_preview_callback(path: &str) -> bool {
    path == PREVIEW_CALLBACK_PATH
}

pub(super) fn request_destination(path_and_query: &str) -> String {
    if path_and_query.is_empty() {
        "/".to_owned()
    } else {
        path_and_query.to_owned()
    }
}

pub(super) fn preview_cookie_value(cookie_header: &str) -> Option<&str> {
    [SECURE_PREVIEW_COOKIE, INSECURE_PREVIEW_COOKIE]
        .into_iter()
        .find_map(|expected| {
            cookie_header.split(';').find_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (name == expected).then_some(value)
            })
        })
}

pub(super) fn strip_preview_cookie(cookie_header: &str) -> Option<String> {
    let cookies = cookie_header
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let (name, _) = pair.split_once('=')?;
            (![SECURE_PREVIEW_COOKIE, INSECURE_PREVIEW_COOKIE].contains(&name)).then_some(pair)
        })
        .collect::<Vec<_>>();
    (!cookies.is_empty()).then(|| cookies.join("; "))
}

pub(super) fn preview_access_cookie(grant: &str, max_age_seconds: u64, secure: bool) -> String {
    let (name, secure_attribute) = if secure {
        (SECURE_PREVIEW_COOKIE, "; Secure")
    } else {
        (INSECURE_PREVIEW_COOKIE, "")
    };
    format!(
        "{name}={grant}; Path=/; Max-Age={max_age_seconds}{secure_attribute}; HttpOnly; SameSite=Lax"
    )
}

pub(super) fn clear_preview_cookies() -> Vec<String> {
    vec![
        format!("{SECURE_PREVIEW_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"),
        format!("{INSECURE_PREVIEW_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
    ]
}

pub(super) fn preview_cache_key(host: &str, grant: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(host.as_bytes());
    digest.update([0]);
    digest.update(grant.as_bytes());
    hex::encode(digest.finalize())
}

pub(super) fn callback_code(request: &Request) -> Option<String> {
    let mut codes = request
        .uri()
        .query()
        .into_iter()
        .flat_map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .filter(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned());
    let code = codes.next().filter(|code| !code.is_empty())?;
    codes.next().is_none().then_some(code)
}

pub(super) fn redirect_response(location: &str, cookies: Vec<String>) -> Response {
    let Ok(location) = HeaderValue::from_str(location) else {
        return error_page(
            StatusCode::BAD_GATEWAY,
            "The control plane returned an invalid authorization redirect.",
        );
    };
    let mut response = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer");
    for cookie in cookies {
        let Ok(cookie) = HeaderValue::from_str(&cookie) else {
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane returned an invalid preview grant.",
            );
        };
        response = response.header(header::SET_COOKIE, cookie);
    }
    response
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn begin_preview_authorization(
    state: &ServeState,
    host: &str,
    return_to: &str,
    clear_cookie: bool,
) -> Response {
    match state
        .client
        .start_preview_authorization(host, return_to)
        .await
    {
        Ok(started) => redirect_response(
            &started.authorization_url,
            clear_cookie.then(clear_preview_cookies).unwrap_or_default(),
        ),
        Err(error) => {
            warn!(
                operation = "node.serve.preview_authorize",
                %error,
                host = %host,
                "preview authorization could not start"
            );
            error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not authorize this preview.",
            )
        }
    }
}

pub(super) async fn handle_preview_callback(
    state: &ServeState,
    host: &str,
    code: Option<String>,
) -> Response {
    let Some(code) = code else {
        return begin_preview_authorization(state, host, "/", true).await;
    };
    match state.client.exchange_preview_code(host, &code).await {
        Ok(exchanged) => redirect_response(
            &exchanged.return_to,
            vec![preview_access_cookie(
                &exchanged.grant,
                exchanged.max_age_seconds.min(12 * 60 * 60),
                exchanged.cookie_secure,
            )],
        ),
        Err(PreviewAuthError::Unauthorized) => {
            begin_preview_authorization(state, host, "/", true).await
        }
        Err(PreviewAuthError::Forbidden) => error_page(
            StatusCode::FORBIDDEN,
            "Your account is not a member of the team that owns this preview.",
        ),
        Err(PreviewAuthError::Infrastructure(error)) => {
            warn!(
                operation = "node.serve.preview_exchange",
                %error,
                host = %host,
                "preview callback exchange failed"
            );
            error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not complete preview authorization.",
            )
        }
    }
}

#[allow(clippy::result_large_err)]
pub(super) async fn require_preview_access(
    state: &ServeState,
    host: &str,
    destination: String,
    grant: Option<String>,
) -> Result<(), Response> {
    let Some(grant) = grant else {
        return Err(begin_preview_authorization(state, host, &destination, false).await);
    };

    let cache_key = preview_cache_key(host, &grant);
    {
        let mut grants = state.preview_grants.lock().await;
        if grants
            .get(&cache_key)
            .is_some_and(|expires_at| *expires_at > Instant::now())
        {
            return Ok(());
        }
        grants.remove(&cache_key);
    }

    match state.client.verify_preview_grant(host, &grant).await {
        Ok(verification) if verification.allowed => {
            let mut grants = state.preview_grants.lock().await;
            let now = Instant::now();
            grants.retain(|_, expires_at| *expires_at > now);
            grants.insert(cache_key, now + state.preview_access_ttl);
            Ok(())
        }
        Ok(_) | Err(PreviewAuthError::Forbidden) => Err(error_page(
            StatusCode::FORBIDDEN,
            "Your account is not a member of the team that owns this preview.",
        )),
        Err(PreviewAuthError::Unauthorized) => {
            Err(begin_preview_authorization(state, host, &destination, true).await)
        }
        Err(PreviewAuthError::Infrastructure(error)) => {
            warn!(
                operation = "node.serve.preview_verify",
                %error,
                host = %host,
                "preview grant verification failed"
            );
            Err(error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not verify preview access.",
            ))
        }
    }
}

pub(super) fn strip_preview_cookie_header(headers: &mut HeaderMap) {
    let filtered = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(strip_preview_cookie);
    match filtered.and_then(|value| HeaderValue::from_str(&value).ok()) {
        Some(value) => {
            headers.insert(header::COOKIE, value);
        }
        None => {
            headers.remove(header::COOKIE);
        }
    }
}
