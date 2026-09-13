use axum::{
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    domain::preview_access::{
        AuthorizationStateRecord, CALLBACK_CODE_TTL, CallbackCodeRecord, configured_site_url,
        expires_after, get_record, is_expired, preview_callback_url, preview_cookie_secure,
        require_current_binding, site_route_url, store_record, take_record,
        user_can_access_preview,
    },
    infra::{error::AppError, http::extractors::session::OptionalSession},
    state::ControlApiState,
};

fn redirect_response(location: String) -> Result<Response, AppError> {
    let location = HeaderValue::from_str(&location).map_err(|_| AppError::Internal {
        op: "preview_auth.redirect",
        message: "preview authorization redirect is invalid".to_owned(),
    })?;
    Ok(Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(axum::body::Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}

fn browser_authorization_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

pub async fn browser_authorization_headers(response: Response) -> Response {
    browser_authorization_response(response)
}

fn forbidden_page() -> Response {
    let response = (
        StatusCode::FORBIDDEN,
        Html("<!doctype html><html><head><meta charset=\"utf-8\"><title>403</title></head><body><h1>403</h1><p>You do not have access to this preview.</p></body></html>"),
    )
        .into_response();
    browser_authorization_response(response)
}

#[derive(Deserialize)]
pub struct AuthorizeQuery {
    state: String,
}

/// GET /api/v1/preview/authorize?state=...
pub async fn authorize(
    State(state): State<ControlApiState>,
    OptionalSession(session): OptionalSession,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Response, AppError> {
    const OP: &str = "preview.authorize";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;

    if session.is_none() {
        let record =
            get_record::<AuthorizationStateRecord>(cache, "authorization", &query.state, OP)
                .await?
                .filter(|record| !is_expired(record.expires_at))
                .ok_or_else(|| AppError::Unauthorized {
                    op: OP,
                    message: "preview authorization is invalid or expired".to_owned(),
                })?;
        require_current_binding(db, &record.binding, OP).await?;
        let site_url = configured_site_url(db, OP).await?;
        let continuation = site_route_url(
            &site_url,
            "/api/v1/preview/authorize",
            "state",
            &query.state,
        )
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        let continuation = url::Url::parse(&continuation)
            .ok()
            .map(|url| format!("{}?{}", url.path(), url.query().unwrap_or_default()))
            .ok_or_else(|| AppError::Internal {
                op: OP,
                message: "preview authorization continuation is invalid".to_owned(),
            })?;
        let login_url = site_route_url(&site_url, "/login", "continue", &continuation)
            .map_err(|source| AppError::Infrastructure { op: OP, source })?;
        return redirect_response(login_url);
    }

    let session = session.expect("checked above");
    let record = take_record::<AuthorizationStateRecord>(cache, "authorization", &query.state, OP)
        .await?
        .filter(|record| !is_expired(record.expires_at))
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview authorization is invalid or expired".to_owned(),
        })?;
    require_current_binding(db, &record.binding, OP).await?;
    if !user_can_access_preview(db, record.binding.team_id, session.data.user_id, OP).await? {
        return Ok(forbidden_page());
    }

    let site_url = configured_site_url(db, OP).await?;
    let cookie_secure = preview_cookie_secure(&site_url)
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    let code = grass_token::generate_token();
    store_record(
        cache,
        "callback",
        &code,
        &CallbackCodeRecord {
            binding: record.binding.clone(),
            return_to: record.return_to,
            session_id: session.session_id,
            user_id: session.data.user_id,
            expires_at: expires_after(CALLBACK_CODE_TTL),
            cookie_secure,
        },
        CALLBACK_CODE_TTL,
        OP,
    )
    .await?;
    let callback = preview_callback_url(&site_url, &record.binding.host, &code)
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    redirect_response(callback)
}

pub(crate) fn router(state: ControlApiState) -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/preview/authorize",
        axum::routing::get(authorize)
            .layer(axum::middleware::from_fn_with_state(
                state,
                crate::infra::http::middlewares::setup_mode::require_ready_mode,
            ))
            .layer(axum::middleware::map_response(
                browser_authorization_headers,
            )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_authorization_failures_are_not_cached_or_referred() {
        let response = forbidden_page();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    }
    #[test]
    fn browser_authorization_error_responses_are_not_cached_or_referred() {
        let response = browser_authorization_response(
            AppError::Unauthorized {
                op: "test.preview_authorize",
                message: "invalid state".to_owned(),
            }
            .into_response(),
        );

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    }
}
