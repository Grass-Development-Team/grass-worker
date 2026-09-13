use axum::{
    Json,
    extract::{ConnectInfo, State},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use grass_cache::Cache;
use serde::Deserialize;
use std::{net::IpAddr, time::Duration};
use uuid::Uuid;

use crate::{
    domain::users,
    infra::{
        database::entity::{user, user_mfa_factor},
        error::{AppError, ok_response},
        http::session_cookies::session_cookie,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/login", axum::routing::post(handler))
}

fn user_data(user: &user::Model) -> UserResponse {
    UserResponse {
        id: user.id,
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        avatar_url: user_avatar_url(user.id, user.avatar_version),
        platform_role: user.platform_role.as_str(),
        email_verified: user.email_verified_at.is_some(),
    }
}

const LOGIN_RATE_PERIOD: Duration = Duration::from_secs(60);

const LOGIN_ACCOUNT_CAPACITY: u32 = 5;

const LOGIN_IP_CAPACITY: u32 = 30;

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
    return_to: Option<String>,
}

async fn handler(
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
    if let Some(response) = begin_login(&state, &user, body.return_to.as_deref()).await? {
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

async fn authenticated_response(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: crate::infra::database::entity::user::Model,
) -> Result<Response, AppError> {
    let (session_jar, csrf_token) = create_authenticated_session(state, cache, jar, &user).await?;

    Ok((
        session_jar,
        ok_response(AuthenticatedResponse {
            user: user_data(&user),
            csrf_token,
        }),
    )
        .into_response())
}

async fn create_authenticated_session(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    jar: CookieJar,
    user: &crate::infra::database::entity::user::Model,
) -> Result<(CookieJar, String), AppError> {
    let issued = crate::domain::authenticated_sessions::issue(state, cache, user).await?;
    let config = state.config.read().unwrap();
    let cookie = session_cookie(
        issued.session_id,
        config.session.cookie_secure,
        config.development_enabled(),
        issued.ttl,
    );
    Ok((jar.add(cookie), issued.csrf_token))
}

async fn begin_login(
    state: &ControlApiState,
    user: &user::Model,
    return_to: Option<&str>,
) -> Result<Option<Response>, AppError> {
    Ok(begin_login_payload(state, user, return_to)
        .await?
        .map(|payload| ok_response(payload).into_response()))
}

async fn begin_login_payload(
    state: &ControlApiState,
    user: &user::Model,
    return_to: Option<&str>,
) -> Result<Option<LoginChallengeResponse>, AppError> {
    Ok(
        crate::domain::login_challenges::begin(state, user, return_to)
            .await?
            .map(|offer| LoginChallengeResponse {
                mfa_required: offer.mode == crate::domain::login_challenges::ChallengeMode::Verify,
                mfa_enrollment_required: offer.mode
                    == crate::domain::login_challenges::ChallengeMode::Enroll,
                challenge_token: offer.challenge_token,
                factors: offer.factors.iter().map(factor_view).collect(),
                allowed_factors: offer.allowed_factors,
                return_to: offer.return_to,
            }),
    )
}

fn factor_view(factor: &user_mfa_factor::Model) -> MfaFactorResponse {
    MfaFactorResponse {
        id: factor.id,
        kind: factor.kind.as_str(),
        label: factor.label.clone(),
        verified: factor.verified_at.is_some(),
        created_at: factor.created_at,
        last_used_at: factor.last_used_at,
    }
}

fn user_avatar_url(user_id: Uuid, version: Option<Uuid>) -> Option<String> {
    version.map(|version| format!("/api/v1/avatars/users/{user_id}/{version}/avatar.webp"))
}

#[derive(serde::Serialize)]
struct UserResponse {
    id: uuid::Uuid,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    platform_role: &'static str,
    email_verified: bool,
}

#[derive(serde::Serialize)]
struct LoginChallengeResponse {
    mfa_required: bool,
    mfa_enrollment_required: bool,
    challenge_token: String,
    factors: Vec<MfaFactorResponse>,
    allowed_factors: Vec<String>,
    return_to: String,
}

#[derive(serde::Serialize)]
struct MfaFactorResponse {
    id: uuid::Uuid,
    kind: &'static str,
    label: Option<String>,
    verified: bool,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_used_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct AuthenticatedResponse {
    user: UserResponse,
    csrf_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::error::AppError;
    use grass_cache::CacheBackend;
    use grass_cache::CacheStore;
    use std::net::IpAddr;
    use std::net::Ipv4Addr;

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
}
