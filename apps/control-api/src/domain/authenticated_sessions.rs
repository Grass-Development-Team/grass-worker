//! Authenticated session issuance and last-login tracking.
use std::time::Duration;

use crate::{
    domain::users,
    infra::{error::AppError, http::middlewares::csrf},
    state::ControlApiState,
};

pub(crate) struct IssuedSession {
    pub(crate) session_id: String,
    pub(crate) csrf_token: String,
    pub(crate) ttl: Duration,
}

pub(crate) async fn issue(
    state: &ControlApiState,
    cache: &grass_cache::CacheStore,
    user: &crate::infra::database::entity::user::Model,
) -> Result<IssuedSession, AppError> {
    if let Some(db) = state.try_database() {
        users::update_last_login(db, user.id)
            .await
            .map_err(|source| AppError::Infrastructure {
                op: "auth.login.update_last_login",
                source,
            })?;
    }
    let session_ttl = Duration::from_secs(state.config.read().unwrap().session.session_ttl_seconds);
    let session_id = grass_session::create_session(cache, user.id, user.auth_version, session_ttl)
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

    Ok(IssuedSession {
        session_id,
        csrf_token,
        ttl: session_ttl,
    })
}
