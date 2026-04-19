use serde::Serialize;
use uuid::Uuid;

pub const SESSION_COOKIE_NAME: &str = "gw_session";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub email: String,
    pub is_admin: bool,
    pub is_initial_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub user: AuthenticatedUser,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthErrorKind {
    Validation,
    Unauthorized,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthError {
    kind: AuthErrorKind,
    message: String,
}

impl AuthError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: AuthErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            kind: AuthErrorKind::Unauthorized,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: AuthErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &AuthErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
