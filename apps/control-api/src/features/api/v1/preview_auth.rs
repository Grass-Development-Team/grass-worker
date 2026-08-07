use std::time::Duration;

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use grass_cache::Cache;
use grass_node_protocol::{
    ExchangePreviewCodeRequest, ExchangePreviewCodeResponse, StartPreviewAuthorizationRequest,
    StartPreviewAuthorizationResponse, VerifyPreviewGrantRequest, VerifyPreviewGrantResponse,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{delivery, deployments, projects, settings, teams, users},
    infra::{
        database::entity::{
            DeploymentBuildStatus, DeploymentServeStatus, PlatformRole, UserStatus,
        },
        error::{AppError, ok_response},
        http::{extractors::session::OptionalSession, middlewares::node_auth::AuthenticatedNode},
    },
    state::ControlApiState,
};

const AUTHORIZATION_STATE_TTL: Duration = Duration::from_secs(5 * 60);
const CALLBACK_CODE_TTL: Duration = Duration::from_secs(60);
const PREVIEW_GRANT_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const SCREENSHOT_GRANT_TTL: Duration = Duration::from_secs(2 * 60);
const SECURE_PREVIEW_COOKIE: &str = "__Host-gw_preview_access";
const INSECURE_PREVIEW_COOKIE: &str = "gw_preview_access";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PreviewBinding {
    deployment_id: Uuid,
    project_id: Uuid,
    team_id: Uuid,
    host: String,
}

#[derive(Serialize, Deserialize)]
struct AuthorizationStateRecord {
    binding: PreviewBinding,
    return_to: String,
    expires_at: i64,
}

#[derive(Serialize, Deserialize)]
struct CallbackCodeRecord {
    binding: PreviewBinding,
    return_to: String,
    session_id: String,
    user_id: Uuid,
    expires_at: i64,
    #[serde(default = "secure_cookie_by_default")]
    cookie_secure: bool,
}

#[derive(Serialize, Deserialize)]
struct PreviewGrantRecord {
    binding: PreviewBinding,
    session_id: String,
    user_id: Uuid,
    expires_at: i64,
}

#[derive(Serialize, Deserialize)]
struct ScreenshotGrantRecord {
    binding: PreviewBinding,
    expires_at: i64,
}

pub(crate) struct ScreenshotPreviewGrant {
    pub target_url: String,
    pub cookie_name: String,
    pub token: String,
}

fn token_key(kind: &str, token: &str) -> String {
    format!("preview:{kind}:{}", grass_token::hash_token(token))
}

fn expires_after(ttl: Duration) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64
}

fn is_expired(expires_at: i64) -> bool {
    expires_at <= OffsetDateTime::now_utc().unix_timestamp()
}

async fn store_record<T: Serialize>(
    cache: &impl Cache,
    kind: &str,
    token: &str,
    record: &T,
    ttl: Duration,
    op: &'static str,
) -> Result<(), AppError> {
    let value = serde_json::to_string(record).map_err(|source| AppError::Infrastructure {
        op,
        source: source.into(),
    })?;
    cache
        .set(&token_key(kind, token), &value, ttl)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })
}

fn parse_record<T: DeserializeOwned>(value: String, op: &'static str) -> Result<T, AppError> {
    serde_json::from_str(&value).map_err(|source| AppError::Infrastructure {
        op,
        source: source.into(),
    })
}

async fn get_record<T: DeserializeOwned>(
    cache: &impl Cache,
    kind: &str,
    token: &str,
    op: &'static str,
) -> Result<Option<T>, AppError> {
    cache
        .get(&token_key(kind, token))
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .map(|value| parse_record(value, op))
        .transpose()
}

async fn take_record<T: DeserializeOwned>(
    cache: &impl Cache,
    kind: &str,
    token: &str,
    op: &'static str,
) -> Result<Option<T>, AppError> {
    cache
        .take(&token_key(kind, token))
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .map(|value| parse_record(value, op))
        .transpose()
}

fn validate_return_to(value: &str) -> anyhow::Result<String> {
    if value.is_empty()
        || value.len() > 4096
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains('\\')
        || value.contains('#')
        || value.chars().any(char::is_control)
    {
        anyhow::bail!("return destination must be a safe relative path and query");
    }
    let uri: Uri = value
        .parse()
        .map_err(|_| anyhow::anyhow!("return destination is not a valid URI"))?;
    if uri.scheme().is_some() || uri.authority().is_some() || uri.path_and_query().is_none() {
        anyhow::bail!("return destination must not contain a scheme or authority");
    }
    Ok(value.to_owned())
}

fn parsed_site_url(site_url: &str) -> anyhow::Result<url::Url> {
    let url = url::Url::parse(site_url)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        anyhow::bail!("site.url must be an absolute http or https URL");
    }
    Ok(url)
}

fn secure_cookie_by_default() -> bool {
    true
}

fn preview_cookie_secure(site_url: &str) -> anyhow::Result<bool> {
    Ok(parsed_site_url(site_url)?.scheme() == "https")
}

fn site_route_url(site_url: &str, path: &str, key: &str, value: &str) -> anyhow::Result<String> {
    let mut url = parsed_site_url(site_url)?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut().append_pair(key, value);
    Ok(url.into())
}

fn preview_callback_url(site_url: &str, host: &str, code: &str) -> anyhow::Result<String> {
    let site = parsed_site_url(site_url)?;
    let host = grass_validator::normalize_host(host)?;
    let mut callback = url::Url::parse(&format!("{}://{host}", site.scheme()))?;
    callback.set_path("/.grass/auth/callback");
    callback.query_pairs_mut().append_pair("code", code);
    Ok(callback.into())
}

async fn configured_site_url(
    db: &sea_orm::DatabaseConnection,
    op: &'static str,
) -> Result<String, AppError> {
    settings::get_setting(db, "site.url")
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .and_then(|setting| setting.value.as_str().map(str::to_owned))
        .ok_or_else(|| AppError::Internal {
            op,
            message: "site.url is not configured".to_owned(),
        })
}

pub(crate) async fn issue_screenshot_grant(
    state: &ControlApiState,
    host: &str,
) -> Result<ScreenshotPreviewGrant, AppError> {
    const OP: &str = "preview.screenshot_grant";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "cache service not available".to_owned(),
    })?;
    let binding = resolve_preview_binding(db, host, OP).await?;
    let site_url = configured_site_url(db, OP).await?;
    let scheme = parsed_site_url(&site_url)
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
        .scheme()
        .to_owned();
    let token = grass_token::generate_token();
    store_record(
        cache,
        "screenshot-grant",
        &token,
        &ScreenshotGrantRecord {
            binding: binding.clone(),
            expires_at: expires_after(SCREENSHOT_GRANT_TTL),
        },
        SCREENSHOT_GRANT_TTL,
        OP,
    )
    .await?;
    Ok(ScreenshotPreviewGrant {
        target_url: format!("{scheme}://{}", binding.host),
        cookie_name: if scheme == "https" {
            SECURE_PREVIEW_COOKIE.to_owned()
        } else {
            INSECURE_PREVIEW_COOKIE.to_owned()
        },
        token,
    })
}

async fn resolve_preview_binding(
    db: &sea_orm::DatabaseConnection,
    raw_host: &str,
    op: &'static str,
) -> Result<PreviewBinding, AppError> {
    let host = grass_validator::normalize_host(raw_host).map_err(|error| AppError::Validation {
        op,
        message: error.to_string(),
    })?;
    let deployment = deployments::find_by_preview_host(db, &host)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .filter(|deployment| {
            matches!(deployment.build_status, DeploymentBuildStatus::Ready)
                && matches!(deployment.serve_status, DeploymentServeStatus::Ready)
        })
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "preview deployment is not ready".to_owned(),
        })?;
    let effective =
        delivery::effective_preview(db, deployment.project_id, deployment.environment.clone())
            .await
            .map_err(|source| AppError::Infrastructure {
                op,
                source: source.into(),
            })?;
    if effective.as_ref().map(|item| item.id) != Some(deployment.id) {
        return Err(AppError::NotFound {
            op,
            message: "preview deployment has been superseded".to_owned(),
        });
    }
    let project = projects::get_by_id_any(db, deployment.project_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "preview project not found".to_owned(),
        })?;
    Ok(PreviewBinding {
        deployment_id: deployment.id,
        project_id: deployment.project_id,
        team_id: project.team_id,
        host,
    })
}

async fn require_current_binding(
    db: &sea_orm::DatabaseConnection,
    expected: &PreviewBinding,
    op: &'static str,
) -> Result<(), AppError> {
    validate_current_binding(
        expected,
        resolve_preview_binding(db, &expected.host, op).await,
        op,
    )
}

fn validate_current_binding(
    expected: &PreviewBinding,
    current: Result<PreviewBinding, AppError>,
    op: &'static str,
) -> Result<(), AppError> {
    let current = match current {
        Ok(current) => current,
        Err(AppError::NotFound { .. }) => {
            return Err(AppError::Unauthorized {
                op,
                message: "preview authorization is invalid or expired".to_owned(),
            });
        }
        Err(error) => return Err(error),
    };
    if current != *expected {
        return Err(AppError::Unauthorized {
            op,
            message: "preview authorization is invalid or expired".to_owned(),
        });
    }
    Ok(())
}

async fn user_is_member(
    db: &sea_orm::DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
    op: &'static str,
) -> Result<bool, AppError> {
    teams::member_role(db, team_id, user_id)
        .await
        .map(|role| role.is_some())
        .map_err(|source| AppError::Infrastructure { op, source })
}

fn preview_access_allowed(
    is_active_user: bool,
    is_team_member: bool,
    is_platform_admin: bool,
) -> bool {
    is_active_user && (is_team_member || is_platform_admin)
}

async fn user_can_access_preview(
    db: &sea_orm::DatabaseConnection,
    team_id: Uuid,
    user_id: Uuid,
    op: &'static str,
) -> Result<bool, AppError> {
    let Some(user) = users::get_user_by_id(db, user_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    else {
        return Ok(false);
    };
    let is_active_user = matches!(user.status, UserStatus::Active);
    if !is_active_user {
        return Ok(false);
    }
    let is_team_member = user_is_member(db, team_id, user_id, op).await?;
    let is_platform_admin = matches!(user.platform_role, PlatformRole::Admin);
    Ok(preview_access_allowed(
        is_active_user,
        is_team_member,
        is_platform_admin,
    ))
}

async fn validate_source_session(
    state: &ControlApiState,
    session_id: &str,
    op: &'static str,
) -> Result<Option<grass_session::SessionData>, AppError> {
    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op,
        message: "cache service not available".to_owned(),
    })?;
    let (idle_ttl, absolute_ttl) = {
        let config = state.config.read().unwrap();
        (
            Duration::from_secs(config.session.idle_ttl_seconds),
            Duration::from_secs(config.session.session_ttl_seconds),
        )
    };
    grass_session::validate_session(cache, session_id, idle_ttl, absolute_ttl)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })
}

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

/// POST /api/v1/internal/serve/preview/authorize
pub async fn start(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<StartPreviewAuthorizationRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_authorize";
    let db = super::internal::database(&state, OP)?;
    let cache = super::internal::cache(&state, OP)?;
    let binding = resolve_preview_binding(db, &body.host, OP).await?;
    let return_to = validate_return_to(&body.return_to).map_err(|error| AppError::Validation {
        op: OP,
        message: error.to_string(),
    })?;
    let token = grass_token::generate_token();
    store_record(
        cache,
        "authorization",
        &token,
        &AuthorizationStateRecord {
            binding,
            return_to,
            expires_at: expires_after(AUTHORIZATION_STATE_TTL),
        },
        AUTHORIZATION_STATE_TTL,
        OP,
    )
    .await?;
    let site_url = configured_site_url(db, OP).await?;
    let authorization_url = site_route_url(&site_url, "/api/v1/preview/authorize", "state", &token)
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(StartPreviewAuthorizationResponse { authorization_url }).into_response())
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

/// POST /api/v1/internal/serve/preview/exchange
pub async fn exchange(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<ExchangePreviewCodeRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_exchange";
    let db = super::internal::database(&state, OP)?;
    let cache = super::internal::cache(&state, OP)?;
    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    let record = take_record::<CallbackCodeRecord>(cache, "callback", &body.code, OP)
        .await?
        .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview callback code is invalid or expired".to_owned(),
        })?;
    require_current_binding(db, &record.binding, OP).await?;
    let session = validate_source_session(&state, &record.session_id, OP)
        .await?
        .filter(|session| session.user_id == record.user_id)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        })?;
    if !user_can_access_preview(db, record.binding.team_id, record.user_id, OP).await? {
        return Err(AppError::Forbidden {
            op: OP,
            message: "preview access requires team membership or platform administration"
                .to_owned(),
        });
    }

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let absolute_session_expiry = session.created_at.unix_timestamp()
        + state.config.read().unwrap().session.session_ttl_seconds as i64;
    let max_age_seconds = PREVIEW_GRANT_TTL
        .as_secs()
        .min((absolute_session_expiry - now).max(0) as u64);
    if max_age_seconds == 0 {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        });
    }

    let grant = grass_token::generate_token();
    store_record(
        cache,
        "grant",
        &grant,
        &PreviewGrantRecord {
            binding: record.binding,
            session_id: record.session_id,
            user_id: record.user_id,
            expires_at: now + max_age_seconds as i64,
        },
        Duration::from_secs(max_age_seconds),
        OP,
    )
    .await?;
    Ok(ok_response(ExchangePreviewCodeResponse {
        grant,
        return_to: record.return_to,
        max_age_seconds,
        cookie_secure: record.cookie_secure,
    })
    .into_response())
}

/// POST /api/v1/internal/serve/preview/verify
pub async fn verify(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(_node)): Extension<AuthenticatedNode>,
    Json(body): Json<VerifyPreviewGrantRequest>,
) -> Result<Response, AppError> {
    const OP: &str = "internal.serve.preview_verify";
    let db = super::internal::database(&state, OP)?;
    let cache = super::internal::cache(&state, OP)?;
    let host =
        grass_validator::normalize_host(&body.host).map_err(|error| AppError::Validation {
            op: OP,
            message: error.to_string(),
        })?;
    if let Some(record) =
        get_record::<ScreenshotGrantRecord>(cache, "screenshot-grant", &body.grant, OP)
            .await?
            .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
    {
        require_current_binding(db, &record.binding, OP).await?;
        return Ok(ok_response(VerifyPreviewGrantResponse { allowed: true }).into_response());
    }
    let record = get_record::<PreviewGrantRecord>(cache, "grant", &body.grant, OP)
        .await?
        .filter(|record| !is_expired(record.expires_at) && record.binding.host == host)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview grant is invalid or expired".to_owned(),
        })?;
    require_current_binding(db, &record.binding, OP).await?;
    let session = validate_source_session(&state, &record.session_id, OP)
        .await?
        .filter(|session| session.user_id == record.user_id)
        .ok_or_else(|| AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        })?;
    if session.user_id != record.user_id {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "preview session is invalid or expired".to_owned(),
        });
    }
    if !user_can_access_preview(db, record.binding.team_id, record.user_id, OP).await? {
        return Err(AppError::Forbidden {
            op: OP,
            message: "preview access requires team membership or platform administration"
                .to_owned(),
        });
    }
    Ok(ok_response(VerifyPreviewGrantResponse { allowed: true }).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header;

    #[test]
    fn return_destination_accepts_only_relative_path_and_query() {
        assert_eq!(validate_return_to("/docs?q=1").unwrap(), "/docs?q=1");
        assert_eq!(validate_return_to("/").unwrap(), "/");

        for value in [
            "",
            "docs",
            "//evil.example/path",
            "https://evil.example/path",
            "/path\\segment",
            "/path\nset-cookie:x",
            "/path#fragment",
        ] {
            assert!(validate_return_to(value).is_err(), "accepted {value:?}");
        }
        assert!(validate_return_to(&format!("/{}", "x".repeat(4096))).is_err());
    }

    #[test]
    fn callback_url_uses_configured_scheme_and_resolved_host() {
        assert_eq!(
            preview_callback_url("https://cxcs.page", "guide-abcd.cxcs.page", "opaque").unwrap(),
            "https://guide-abcd.cxcs.page/.grass/auth/callback?code=opaque"
        );
        assert_eq!(
            preview_callback_url("http://localhost:7817", "preview.test", "a b").unwrap(),
            "http://preview.test/.grass/auth/callback?code=a+b"
        );
        assert!(preview_callback_url("file:///tmp", "preview.test", "code").is_err());
    }

    #[test]
    fn preview_cookie_security_follows_the_callback_scheme() {
        assert!(preview_cookie_secure("https://cxcs.page").unwrap());
        assert!(!preview_cookie_secure("http://localhost:7817").unwrap());
    }

    #[test]
    fn protected_previews_allow_team_members_or_platform_admins() {
        assert!(preview_access_allowed(true, true, false));
        assert!(preview_access_allowed(true, false, true));
        assert!(!preview_access_allowed(true, false, false));
        assert!(!preview_access_allowed(false, true, false));
        assert!(!preview_access_allowed(false, false, true));
    }

    #[test]
    fn missing_current_binding_invalidates_preview_authorization() {
        let expected = PreviewBinding {
            deployment_id: Uuid::nil(),
            project_id: Uuid::nil(),
            team_id: Uuid::nil(),
            host: "guide-abcd.cxcs.page".to_owned(),
        };
        let result = validate_current_binding(
            &expected,
            Err(AppError::NotFound {
                op: "test.resolve_preview",
                message: "preview deployment is not ready".to_owned(),
            }),
            "test.preview_binding",
        );

        assert!(matches!(result, Err(AppError::Unauthorized { .. })));
    }

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
