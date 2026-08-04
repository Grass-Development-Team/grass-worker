use std::{net::IpAddr, time::Duration};

use axum::{
    Json,
    extract::{ConnectInfo, State},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use serde_json::json;

use grass_cache::Cache;

use crate::{
    domain::users,
    infra::{
        error::{AppError, ok_response},
        http::middlewares::csrf,
    },
    state::ControlApiState,
};

const LOGIN_RATE_PERIOD: Duration = Duration::from_secs(60);
const LOGIN_ACCOUNT_CAPACITY: u32 = 5;
const LOGIN_IP_CAPACITY: u32 = 30;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub return_to: Option<String>,
}

pub async fn handler(
    State(state): State<ControlApiState>,
    ConnectInfo(peer_address): ConnectInfo<std::net::SocketAddr>,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: "auth.login.no_database",
        message: "database not available".to_owned(),
    })?;

    let cache = state.try_cache().ok_or_else(|| AppError::Internal {
        op: "auth.login.no_cache",
        message: "cache service not available".to_owned(),
    })?;

    let email =
        grass_validator::normalize_email(&body.email).map_err(|error| AppError::Validation {
            op: "auth.login.invalid_email",
            message: error.to_string(),
        })?;

    if body.password.is_empty() || body.password.len() > 1024 {
        return Err(AppError::Validation {
            op: "auth.login.empty_password",
            message: "password must contain between 1 and 1024 bytes".to_owned(),
        });
    }

    enforce_login_rate_limits(cache, &email, peer_address.ip()).await?;

    let user = users::verify_user_password(db, &email, &body.password)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.verify",
            source,
        })?
        .ok_or_else(|| AppError::Unauthorized {
            op: "auth.login.invalid_credentials",
            message: "invalid email or password".to_owned(),
        })?;
    if user.email_verified_at.is_none() {
        return Err(AppError::Forbidden {
            op: "auth.login.email_not_verified",
            message: "email verification is required".to_owned(),
        });
    }
    if let Some(response) =
        super::mfa::begin_login(&state, &user, body.return_to.as_deref()).await?
    {
        return Ok(response);
    }

    authenticated_response(&state, cache, jar, user).await
}

async fn enforce_login_rate_limits(
    cache: &impl Cache,
    email: &str,
    source_ip: IpAddr,
) -> Result<(), AppError> {
    let account_key = format!("rate:login:account:{}", grass_token::hash_token(email));
    let ip_key = format!("rate:login:ip:{source_ip}");

    let account_allowed = cache
        .consume_rate_limit(&account_key, LOGIN_ACCOUNT_CAPACITY, LOGIN_RATE_PERIOD)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.account_rate_limit",
            source,
        })?;
    let ip_allowed = cache
        .consume_rate_limit(&ip_key, LOGIN_IP_CAPACITY, LOGIN_RATE_PERIOD)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.ip_rate_limit",
            source,
        })?;

    if !account_allowed || !ip_allowed {
        return Err(AppError::TooManyRequests {
            op: "auth.login.rate_limited",
            message: "too many login attempts; try again later".to_owned(),
        });
    }
    Ok(())
}

pub(crate) async fn authenticated_response(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: crate::infra::database::entity::user::Model,
) -> Result<Response, AppError> {
    let (session_jar, csrf_token) = create_authenticated_session(state, cache, jar, &user).await?;

    Ok((
        session_jar,
        ok_response(json!({
            "user": super::user_data(&user),
            "csrf_token": csrf_token,
        })),
    )
        .into_response())
}

pub(crate) async fn create_authenticated_session(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: &crate::infra::database::entity::user::Model,
) -> Result<(CookieJar, String), AppError> {
    if let Some(db) = state.try_database() {
        users::update_last_login(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "auth.login.update_last_login",
                source,
            })?;
    }
    let (cookie_secure, session_ttl) = {
        let config = state.config.read().unwrap();
        (
            config.session.cookie_secure,
            Duration::from_secs(config.session.session_ttl_seconds),
        )
    };
    let session_id = grass_session::create_session(cache, user.id, session_ttl)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.create_session",
            source,
        })?;

    let csrf_token = csrf::generate_csrf_token(cache, &session_id)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: "auth.login.csrf_token",
            source,
        })?;

    Ok((
        jar.add(session_cookie(session_id, cookie_secure, session_ttl)),
        csrf_token,
    ))
}

fn session_cookie(
    session_id: impl Into<String>,
    secure: bool,
    session_ttl: Duration,
) -> Cookie<'static> {
    let mut cookie = Cookie::new("session_id", session_id.into());
    cookie.set_path("/api");
    cookie.set_http_only(true);
    cookie.set_secure(secure);
    if secure {
        cookie.set_partitioned(true);
    }
    cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    cookie.set_max_age(time::Duration::seconds(session_ttl.as_secs() as i64));
    cookie
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use grass_cache::{CacheBackend, CacheStore};

    use super::*;

    #[tokio::test]
    async fn login_rate_limits_accounts_and_source_addresses() {
        let cache = CacheStore::connect_cache(CacheBackend::Moka, "")
            .await
            .unwrap();
        let first_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
        for _ in 0..5 {
            enforce_login_rate_limits(&cache, "user@example.com", first_ip)
                .await
                .unwrap();
        }
        assert!(matches!(
            enforce_login_rate_limits(&cache, "user@example.com", first_ip).await,
            Err(AppError::TooManyRequests { .. })
        ));

        let second_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2));
        for index in 0..30 {
            enforce_login_rate_limits(&cache, &format!("user-{index}@example.com"), second_ip)
                .await
                .unwrap();
        }
        assert!(matches!(
            enforce_login_rate_limits(&cache, "last@example.com", second_ip).await,
            Err(AppError::TooManyRequests { .. })
        ));
    }

    #[test]
    fn session_cookie_contains_all_security_attributes() {
        let cookie = session_cookie("session", true, Duration::from_secs(3600));

        assert_eq!(cookie.path(), Some("/api"));
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(
            cookie.same_site(),
            Some(axum_extra::extract::cookie::SameSite::Strict)
        );
        assert_eq!(cookie.partitioned(), Some(true));
        assert_eq!(cookie.max_age(), Some(time::Duration::hours(1)));
    }
}
