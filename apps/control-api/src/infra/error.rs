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
    Conflict { op: &'static str, message: String },
    #[error("{source}")]
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
            Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Infrastructure { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
            Self::SetupNotAllowed { .. } => StatusCode::FORBIDDEN,
        }
    }

    fn op(&self) -> &'static str {
        match self {
            Self::Validation { op, .. } => op,
            Self::Conflict { op, .. } => op,
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
            Self::Conflict { .. } => 40901,
            Self::Infrastructure { .. } => 50001,
            Self::Internal { .. } => 50099,
            Self::SetupNotAllowed { .. } => 40301,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
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
