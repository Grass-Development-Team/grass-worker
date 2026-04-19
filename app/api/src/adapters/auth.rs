use crate::domain::auth::{
    AuthError, AuthenticatedSession, AuthenticatedUser, LoginInput, SESSION_COOKIE_NAME,
};
use argon2::{Argon2, PasswordVerifier, password_hash::PasswordHash};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use chrono::{Duration, Utc};
use grass_worker_database::{
    entities::{user, user_session},
    repository::{
        find_password_credential_by_user_id, find_session_by_token_hash, find_user_by_email,
        find_user_by_id, insert_session, revoke_session_by_token_hash,
    },
};
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthService;

impl AuthService {
    pub fn write_login_cookie(&self, jar: CookieJar, token: &str) -> CookieJar {
        jar.add(Self::build_session_cookie(token.to_owned()))
    }

    pub async fn login(
        &self,
        database: &DatabaseConnection,
        input: LoginInput,
    ) -> Result<AuthenticatedSession, AuthError> {
        let email = input.email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Err(AuthError::validation("email is required"));
        }
        if input.password.is_empty() {
            return Err(AuthError::validation("password is required"));
        }

        let user = find_user_by_email(database, &email)
            .await
            .map_err(database_error)?
            .ok_or_else(|| AuthError::unauthorized("invalid credentials"))?;
        let credential = find_password_credential_by_user_id(database, user.id)
            .await
            .map_err(database_error)?
            .ok_or_else(|| AuthError::unauthorized("invalid credentials"))?;
        let password_hash = PasswordHash::new(&credential.password_hash)
            .map_err(|error| AuthError::internal(error.to_string()))?;

        Argon2::default()
            .verify_password(input.password.as_bytes(), &password_hash)
            .map_err(|_| AuthError::unauthorized("invalid credentials"))?;

        let token = Uuid::new_v4().to_string();
        let token_hash = hash_session_token(&token);
        let now = Utc::now();

        insert_session(
            database,
            &user_session::Model {
                id: Uuid::new_v4(),
                user_id: user.id,
                token_hash,
                created_at: now,
                expires_at: now + Duration::days(7),
                revoked_at: None,
            },
        )
        .await
        .map_err(database_error)?;

        Ok(AuthenticatedSession {
            user: map_authenticated_user(user),
            token,
        })
    }

    pub async fn current_user(
        &self,
        database: &DatabaseConnection,
        session_token: &str,
    ) -> Result<AuthenticatedUser, AuthError> {
        let token_hash = hash_session_token(session_token);
        let session = find_session_by_token_hash(database, &token_hash)
            .await
            .map_err(database_error)?
            .ok_or_else(|| AuthError::unauthorized("not authenticated"))?;
        let now = Utc::now();

        if session.revoked_at.is_some() || session.expires_at <= now {
            return Err(AuthError::unauthorized("not authenticated"));
        }

        let user = find_user_by_id(database, session.user_id)
            .await
            .map_err(database_error)?
            .ok_or_else(|| AuthError::unauthorized("not authenticated"))?;

        Ok(map_authenticated_user(user))
    }

    pub async fn logout(
        &self,
        database: &DatabaseConnection,
        session_token: Option<&str>,
    ) -> Result<(), AuthError> {
        let Some(session_token) = session_token else {
            return Ok(());
        };
        if session_token.is_empty() {
            return Ok(());
        }

        revoke_session_by_token_hash(database, &hash_session_token(session_token), Utc::now())
            .await
            .map_err(database_error)
    }

    pub fn build_session_cookie(token: impl Into<String>) -> Cookie<'static> {
        Cookie::build((SESSION_COOKIE_NAME, token.into()))
            .http_only(true)
            .same_site(SameSite::Lax)
            .path("/")
            .build()
    }
}

pub fn clear_session_cookie(jar: CookieJar) -> CookieJar {
    let mut cookie = AuthService::build_session_cookie("");
    cookie.make_removal();
    jar.add(cookie)
}

pub fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn map_authenticated_user(user: user::Model) -> AuthenticatedUser {
    AuthenticatedUser {
        id: user.id,
        email: user.email,
        is_admin: user.is_admin,
        is_initial_admin: user.is_initial_admin,
    }
}

fn database_error(error: sea_orm::DbErr) -> AuthError {
    AuthError::internal(error.to_string())
}
