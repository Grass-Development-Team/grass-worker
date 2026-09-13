use axum::http::Uri;
use grass_cache::Cache;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    domain::{delivery, deployments, projects, settings, teams, users},
    infra::{
        database::entity::{
            DeploymentBuildStatus, DeploymentServeStatus, PlatformRole, UserStatus,
        },
        error::AppError,
    },
    state::ControlApiState,
};

pub(crate) const AUTHORIZATION_STATE_TTL: Duration = Duration::from_secs(5 * 60);

pub(crate) const CALLBACK_CODE_TTL: Duration = Duration::from_secs(60);

pub(crate) const PREVIEW_GRANT_TTL: Duration = Duration::from_secs(12 * 60 * 60);

pub(crate) const SCREENSHOT_GRANT_TTL: Duration = Duration::from_secs(2 * 60);

pub(crate) const SECURE_PREVIEW_COOKIE: &str = "__Host-gw_preview_access";

pub(crate) const INSECURE_PREVIEW_COOKIE: &str = "gw_preview_access";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PreviewBinding {
    pub(crate) deployment_id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) team_id: Uuid,
    pub(crate) host: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct AuthorizationStateRecord {
    pub(crate) binding: PreviewBinding,
    pub(crate) return_to: String,
    pub(crate) expires_at: i64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CallbackCodeRecord {
    pub(crate) binding: PreviewBinding,
    pub(crate) return_to: String,
    pub(crate) session_id: String,
    pub(crate) user_id: Uuid,
    pub(crate) expires_at: i64,
    #[serde(default = "secure_cookie_by_default")]
    pub(crate) cookie_secure: bool,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PreviewGrantRecord {
    pub(crate) binding: PreviewBinding,
    pub(crate) session_id: String,
    pub(crate) user_id: Uuid,
    pub(crate) expires_at: i64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ScreenshotGrantRecord {
    pub(crate) binding: PreviewBinding,
    pub(crate) expires_at: i64,
}

pub(crate) struct ScreenshotPreviewGrant {
    pub(crate) target_url: String,
    pub(crate) cookie_name: String,
    pub(crate) token: String,
}

pub(crate) fn token_key(kind: &str, token: &str) -> String {
    format!("preview:{kind}:{}", grass_token::hash_token(token))
}

pub(crate) fn expires_after(ttl: Duration) -> i64 {
    OffsetDateTime::now_utc().unix_timestamp() + ttl.as_secs() as i64
}

pub(crate) fn is_expired(expires_at: i64) -> bool {
    expires_at <= OffsetDateTime::now_utc().unix_timestamp()
}

pub(crate) async fn store_record<T: Serialize>(
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

pub(crate) fn parse_record<T: DeserializeOwned>(
    value: String,
    op: &'static str,
) -> Result<T, AppError> {
    serde_json::from_str(&value).map_err(|source| AppError::Infrastructure {
        op,
        source: source.into(),
    })
}

pub(crate) async fn get_record<T: DeserializeOwned>(
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

pub(crate) async fn take_record<T: DeserializeOwned>(
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

pub(crate) fn validate_return_to(value: &str) -> anyhow::Result<String> {
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

pub(crate) fn parsed_site_url(site_url: &str) -> anyhow::Result<url::Url> {
    let url = url::Url::parse(site_url)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        anyhow::bail!("site.url must be an absolute http or https URL");
    }
    Ok(url)
}

pub(crate) fn secure_cookie_by_default() -> bool {
    true
}

pub(crate) fn preview_cookie_secure(site_url: &str) -> anyhow::Result<bool> {
    Ok(parsed_site_url(site_url)?.scheme() == "https")
}

pub(crate) fn site_route_url(
    site_url: &str,
    path: &str,
    key: &str,
    value: &str,
) -> anyhow::Result<String> {
    let mut url = parsed_site_url(site_url)?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    url.query_pairs_mut().append_pair(key, value);
    Ok(url.into())
}

pub(crate) fn preview_callback_url(
    site_url: &str,
    host: &str,
    code: &str,
) -> anyhow::Result<String> {
    let site = parsed_site_url(site_url)?;
    let host = grass_validator::normalize_host(host)?;
    let mut callback = url::Url::parse(&format!("{}://{host}", site.scheme()))?;
    callback.set_path("/.grass/auth/callback");
    callback.query_pairs_mut().append_pair("code", code);
    Ok(callback.into())
}

pub(crate) async fn configured_site_url(
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
    pub(crate) const OP: &str = "preview.screenshot_grant";
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

pub(crate) async fn resolve_preview_binding(
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

pub(crate) async fn require_current_binding(
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

pub(crate) fn validate_current_binding(
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

pub(crate) async fn user_is_member(
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

pub(crate) fn preview_access_allowed(
    is_active_user: bool,
    is_team_member: bool,
    is_platform_admin: bool,
) -> bool {
    is_active_user && (is_team_member || is_platform_admin)
}

pub(crate) async fn user_can_access_preview(
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

pub(crate) async fn validate_source_session(
    state: &ControlApiState,
    session_id: &str,
    op: &'static str,
) -> Result<Option<grass_session::SessionData>, AppError> {
    crate::infra::http::middlewares::session::validate_current_session(state, session_id, op).await
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
