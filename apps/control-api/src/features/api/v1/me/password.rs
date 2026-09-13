use axum::{Json, extract::State, response::IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use sea_orm::TransactionTrait;
use serde::Deserialize;

use crate::{
    domain::{authentication, users},
    infra::{
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route("/me/password", axum::routing::post(change))
}

pub(super) fn removal_cookie(
    configured_secure: bool,
    development_enabled: bool,
) -> Cookie<'static> {
    let secure = configured_secure && !development_enabled;
    let mut clear_cookie = Cookie::new("session_id", "");
    clear_cookie.set_path("/api");
    clear_cookie.set_http_only(true);
    clear_cookie.set_secure(secure);
    clear_cookie.set_same_site(axum_extra::extract::cookie::SameSite::Strict);
    if secure {
        clear_cookie.set_partitioned(true);
    }
    clear_cookie.make_removal();
    clear_cookie
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub password: String,
}

pub async fn change(
    State(state): State<ControlApiState>,
    Session { data, .. }: Session,
    jar: CookieJar,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "me.password.change";
    let db = state.try_database().ok_or_else(|| AppError::Internal {
        op: OP,
        message: "database not available".to_owned(),
    })?;
    if !authentication::verify_password_for_user(db, data.user_id, &body.current_password)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?
    {
        return Err(AppError::Unauthorized {
            op: OP,
            message: "current password is incorrect".to_owned(),
        });
    }
    let policy = authentication::password_policy(db)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    policy
        .validate_password(&body.password)
        .map_err(|message| AppError::Validation {
            op: OP,
            message: message.to_owned(),
        })?;
    ensure_not_reused(db, data.user_id, &body.password, policy.history_count, OP).await?;
    set_password(db, data.user_id, &body.password, OP).await?;
    let config = state.config.read().unwrap();
    let jar = jar.add(removal_cookie(
        config.session.cookie_secure,
        config.development_enabled(),
    ));
    Ok((jar, ok_response(ChangeResponse { changed: true })))
}

async fn ensure_not_reused(
    db: &sea_orm::DatabaseConnection,
    user_id: uuid::Uuid,
    password: &str,
    count: usize,
    op: &'static str,
) -> Result<(), AppError> {
    if authentication::password_was_used_recently(db, user_id, password, count)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
    {
        return Err(AppError::Validation {
            op,
            message: "password was used recently".to_owned(),
        });
    }
    Ok(())
}

async fn set_password<C: sea_orm::ConnectionTrait + TransactionTrait>(
    db: &C,
    user_id: uuid::Uuid,
    password: &str,
    op: &'static str,
) -> Result<(), AppError> {
    let hash = grass_crypto::hash_password(password).map_err(|error| AppError::Internal {
        op,
        message: format!("password hashing failed: {error}"),
    })?;
    users::set_password(db, user_id, hash)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })
}

#[derive(serde::Serialize)]
struct ChangeResponse {
    changed: bool,
}
