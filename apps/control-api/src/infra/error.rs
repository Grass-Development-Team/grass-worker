use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    Validation { op: &'static str, message: String },
    #[error("{message}")]
    Unauthorized { op: &'static str, message: String },
    #[error("{message}")]
    Forbidden { op: &'static str, message: String },
    #[error("{message}")]
    NotFound { op: &'static str, message: String },
    #[error("{message}")]
    Gone { op: &'static str, message: String },
    #[error("{message}")]
    Conflict { op: &'static str, message: String },
    #[allow(dead_code)]
    #[error("{message}")]
    TooManyRequests { op: &'static str, message: String },
    #[error("{message}")]
    QuotaExceeded { op: &'static str, message: String },
    #[error("infrastructure service unavailable")]
    Infrastructure {
        op: &'static str,
        source: anyhow::Error,
    },
    #[error("{message}")]
    Internal { op: &'static str, message: String },
    #[error("{message}")]
    SetupNotAllowed { op: &'static str, message: String },
}

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: u16,
    pub message: String,
    pub data: T,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: u16,
    pub message: String,
    pub data: serde_json::Value,
    pub op: &'static str,
}

impl AppError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation { .. } => StatusCode::BAD_REQUEST,
            Self::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Gone { .. } => StatusCode::GONE,
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::QuotaExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Infrastructure { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::SetupNotAllowed { .. } => StatusCode::FORBIDDEN,
        }
    }

    fn op(&self) -> &'static str {
        match self {
            Self::Validation { op, .. } => op,
            Self::Unauthorized { op, .. } => op,
            Self::Forbidden { op, .. } => op,
            Self::NotFound { op, .. } => op,
            Self::Gone { op, .. } => op,
            Self::Conflict { op, .. } => op,
            Self::TooManyRequests { op, .. } => op,
            Self::QuotaExceeded { op, .. } => op,
            Self::Infrastructure { op, .. } => op,
            Self::Internal { op, .. } => op,
            Self::SetupNotAllowed { op, .. } => op,
        }
    }

    fn message(&self) -> String {
        self.to_string()
    }

    fn error_code(&self) -> u16 {
        match self {
            Self::Validation { .. } => 40001,
            Self::Unauthorized { .. } => 40101,
            Self::Forbidden { .. } => 40301,
            Self::NotFound { .. } => 40401,
            Self::Gone { .. } => 41001,
            Self::Conflict { .. } => 40901,
            Self::TooManyRequests { .. } => 42901,
            Self::QuotaExceeded { .. } => 42902,
            Self::Infrastructure { .. } => 50001,
            Self::Internal { .. } => 50099,
            Self::SetupNotAllowed { .. } => 40302,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Infrastructure { op, source } = &self {
            tracing::error!(operation = *op, error = %source, "infrastructure request failed");
        }
        let status = self.status_code();
        let body = ErrorBody {
            code: self.error_code(),
            message: self.message(),
            data: serde_json::Value::Null,
            op: self.op(),
        };
        (status, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self::Infrastructure {
            op: "infra.fallback",
            source: error,
        }
    }
}

pub fn ok_response<T: Serialize>(data: T) -> impl IntoResponse {
    Json(ApiResponse {
        code: 200,
        message: "OK".to_string(),
        data,
    })
}

pub fn accepted_response<T: Serialize>(data: T) -> impl IntoResponse {
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse {
            code: 202,
            message: "Accepted".to_owned(),
            data,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infrastructure_errors_do_not_display_their_source() {
        let error = AppError::Infrastructure {
            op: "test.infrastructure",
            source: anyhow::anyhow!("database password and schema details"),
        };

        assert_eq!(error.to_string(), "infrastructure service unavailable");
    }

    #[test]
    fn accepted_response_uses_http_and_envelope_code_202() {
        let response = accepted_response(serde_json::json!({ "queued": true })).into_response();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
}
