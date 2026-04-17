use async_trait::async_trait;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialAdminInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminSetupErrorKind {
    Validation,
    Conflict,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSetupError {
    kind: AdminSetupErrorKind,
    message: String,
}

impl AdminSetupError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Validation,
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Conflict,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            kind: AdminSetupErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> &AdminSetupErrorKind {
        &self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[async_trait]
pub trait AdminSetupService: Send + Sync {
    async fn create_initial_admin(
        &self,
        input: InitialAdminInput,
    ) -> Result<(), AdminSetupError>;
}

pub type SharedAdminSetupService = Arc<dyn AdminSetupService>;

#[derive(Debug)]
pub struct RejectingAdminSetupService;

#[async_trait]
impl AdminSetupService for RejectingAdminSetupService {
    async fn create_initial_admin(
        &self,
        _input: InitialAdminInput,
    ) -> Result<(), AdminSetupError> {
        Err(AdminSetupError::internal(
            "admin setup service is not configured",
        ))
    }
}
