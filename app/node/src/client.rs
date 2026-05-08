use chrono::{DateTime, Utc};
use reqwest::header::{AUTHORIZATION, CONTENT_DISPOSITION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedDeployment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub status: String,
    pub source_branch: String,
    pub source_revision: Option<String>,
    pub last_stage: Option<String>,
    pub failure_message: Option<String>,
    pub repository_url: String,
    pub production_branch: String,
    pub root_directory: Option<String>,
    pub install_command: String,
    pub build_command: String,
    pub output_directory: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct ClaimedDeploymentEnvelope {
    deployment: ClaimedDeployment,
}

#[derive(Debug, Serialize)]
struct UpdateStageRequest<'a> {
    stage: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_message: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneClient {
    base_url: String,
    shared_token: String,
    http: reqwest::Client,
}

#[derive(Debug)]
pub enum ClientError {
    Transport(reqwest::Error),
    Response {
        status: reqwest::StatusCode,
        message: String,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "{error}"),
            Self::Response { status, message } => {
                write!(f, "control plane request failed with {status}: {message}")
            }
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Response { .. } => None,
        }
    }
}

impl ControlPlaneClient {
    pub fn new(base_url: String, shared_token: String) -> Result<Self, ClientError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(ClientError::Transport)?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            shared_token,
            http,
        })
    }

    pub async fn claim_next_deployment(&self) -> Result<Option<ClaimedDeployment>, ClientError> {
        let response = self
            .http
            .post(format!("{}/api/v1/internal/deployments/claim", self.base_url))
            .header(AUTHORIZATION, self.authorization_header())
            .send()
            .await
            .map_err(ClientError::Transport)?;

        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        let response = self.error_for_status(response).await?;
        let envelope = response
            .json::<ClaimedDeploymentEnvelope>()
            .await
            .map_err(ClientError::Transport)?;
        Ok(Some(envelope.deployment))
    }

    pub async fn update_stage(
        &self,
        deployment_id: Uuid,
        stage: &str,
        status: Option<&str>,
        failure_message: Option<&str>,
    ) -> Result<(), ClientError> {
        let response = self
            .http
            .post(format!(
                "{}/api/v1/internal/deployments/{deployment_id}/stage",
                self.base_url
            ))
            .header(AUTHORIZATION, self.authorization_header())
            .json(&UpdateStageRequest {
                stage,
                status,
                failure_message,
            })
            .send()
            .await
            .map_err(ClientError::Transport)?;

        self.error_for_status(response).await?;
        Ok(())
    }

    pub async fn upload_build_log(
        &self,
        deployment_id: Uuid,
        contents: &str,
    ) -> Result<(), ClientError> {
        let response = self
            .http
            .put(format!(
                "{}/api/v1/internal/deployments/{deployment_id}/build-log",
                self.base_url
            ))
            .header(AUTHORIZATION, self.authorization_header())
            .header(CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(contents.to_owned())
            .send()
            .await
            .map_err(ClientError::Transport)?;

        self.error_for_status(response).await?;
        Ok(())
    }

    pub async fn upload_static_site(
        &self,
        deployment_id: Uuid,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), ClientError> {
        let response = self
            .http
            .put(format!(
                "{}/api/v1/internal/deployments/{deployment_id}/static-site",
                self.base_url
            ))
            .header(AUTHORIZATION, self.authorization_header())
            .header(CONTENT_TYPE, "application/zip")
            .header(
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            )
            .body(bytes)
            .send()
            .await
            .map_err(ClientError::Transport)?;

        self.error_for_status(response).await?;
        Ok(())
    }

    fn authorization_header(&self) -> String {
        format!("Bearer {}", self.shared_token)
    }

    async fn error_for_status(
        &self,
        response: reqwest::Response,
    ) -> Result<reqwest::Response, ClientError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let bytes = response.bytes().await.map_err(ClientError::Transport)?;
        let message = serde_json::from_slice::<ErrorResponse>(&bytes)
            .map(|payload| payload.error)
            .unwrap_or_else(|_error| String::from_utf8_lossy(&bytes).into_owned());

        Err(ClientError::Response { status, message })
    }
}
