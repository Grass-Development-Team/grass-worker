//! HTTP client for the Control API internal protocol.

use std::{path::Path, time::Duration};

use anyhow::Context;
use futures_util::StreamExt;
use grass_node_protocol::{
    AppendBuildLogRequest, AppendBuildLogResponse, ClaimRequest, ClaimResponse,
    ExchangePreviewCodeRequest, ExchangePreviewCodeResponse, HeartbeatRequest, HeartbeatResponse,
    ObserveSshHostKeyRequest, ObserveSshHostKeyResponse, RedeemGitCredentialRequest,
    RedeemGitCredentialResponse, RegisterRequest, RegisterResponse, ReportServeStatusRequest,
    ReportServeStatusResponse, ResolveHostResponse, RouteSnapshotResponse, ServeAssignment,
    ServeAssignmentsResponse, StageRequest, StageResponse, StartPreviewAuthorizationRequest,
    StartPreviewAuthorizationResponse, UploadArtifactResponse, VerifyPreviewGrantRequest,
    VerifyPreviewGrantResponse, artifact_headers,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

/// Control API response envelope.
#[derive(Debug, Deserialize)]
struct Envelope<T> {
    code: u16,
    message: String,
    data: Option<T>,
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewAuthError {
    #[error("preview authorization is invalid or expired")]
    Unauthorized,
    #[error("preview access is forbidden")]
    Forbidden,
    #[error(transparent)]
    Infrastructure(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct ControlApiClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl ControlApiClient {
    pub fn new(base_url: &str, token: &str) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
            http,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/internal{path}", self.base_url)
    }

    async fn unwrap_envelope<T: DeserializeOwned>(
        response: reqwest::Response,
        operation: &str,
    ) -> anyhow::Result<T> {
        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("{operation}: failed to read response body"))?;
        let envelope: Envelope<T> = serde_json::from_str(&body)
            .with_context(|| format!("{operation}: invalid response ({status})"))?;
        if envelope.code != 200 {
            anyhow::bail!(
                "{operation}: control api error {}: {}",
                envelope.code,
                envelope.message
            );
        }
        envelope
            .data
            .ok_or_else(|| anyhow::anyhow!("{operation}: response missing data"))
    }

    async fn post_json<Req: serde::Serialize, Res: DeserializeOwned>(
        &self,
        path: &str,
        body: &Req,
        operation: &str,
    ) -> anyhow::Result<Res> {
        let response = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("{operation}: request failed"))?;
        Self::unwrap_envelope(response, operation).await
    }

    async fn post_preview_json<Req: serde::Serialize, Res: DeserializeOwned>(
        &self,
        path: &str,
        body: &Req,
        operation: &str,
    ) -> Result<Res, PreviewAuthError> {
        let response = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("{operation}: request failed"))
            .map_err(PreviewAuthError::Infrastructure)?;
        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED => Err(PreviewAuthError::Unauthorized),
            reqwest::StatusCode::FORBIDDEN => Err(PreviewAuthError::Forbidden),
            _ => Self::unwrap_envelope(response, operation)
                .await
                .map_err(PreviewAuthError::Infrastructure),
        }
    }

    pub async fn register(&self, request: &RegisterRequest) -> anyhow::Result<RegisterResponse> {
        self.post_json("/nodes/register", request, "node.register")
            .await
    }

    pub async fn heartbeat(&self, request: &HeartbeatRequest) -> anyhow::Result<HeartbeatResponse> {
        self.post_json("/nodes/heartbeat", request, "node.heartbeat")
            .await
    }

    #[allow(dead_code)] // Wired by the build loop in Milestone 7.
    pub async fn claim(&self, request: &ClaimRequest) -> anyhow::Result<ClaimResponse> {
        self.post_json("/deployments/claim", request, "deployment.claim")
            .await
    }

    #[allow(dead_code)] // Wired by the build loop in Milestone 7.
    pub async fn report_stage(
        &self,
        deployment_id: Uuid,
        request: &StageRequest,
    ) -> anyhow::Result<StageResponse> {
        self.post_json(
            &format!("/deployments/{deployment_id}/stage"),
            request,
            "deployment.stage",
        )
        .await
    }

    pub async fn redeem_source_credential(
        &self,
        deployment_id: Uuid,
        lease: String,
    ) -> anyhow::Result<RedeemGitCredentialResponse> {
        self.post_json(
            &format!("/deployments/{deployment_id}/source-credential"),
            &RedeemGitCredentialRequest { lease },
            "deployment.source_credential",
        )
        .await
    }

    pub async fn observe_ssh_host_key(
        &self,
        deployment_id: Uuid,
        request: &ObserveSshHostKeyRequest,
    ) -> anyhow::Result<ObserveSshHostKeyResponse> {
        self.post_json(
            &format!("/deployments/{deployment_id}/ssh-host-key"),
            request,
            "deployment.ssh_host_key",
        )
        .await
    }

    #[allow(dead_code)] // Wired by the build loop in Milestone 7.
    pub async fn append_build_log(
        &self,
        deployment_id: Uuid,
        request: &AppendBuildLogRequest,
    ) -> anyhow::Result<AppendBuildLogResponse> {
        let response = self
            .http
            .put(self.url(&format!("/deployments/{deployment_id}/build-log")))
            .bearer_auth(&self.token)
            .json(request)
            .send()
            .await
            .context("deployment.build_log: request failed")?;
        Self::unwrap_envelope(response, "deployment.build_log").await
    }

    #[allow(clippy::too_many_arguments)]
    async fn artifact_upload_request(
        &self,
        deployment_id: Uuid,
        artifact_path: &Path,
        packed_size_bytes: u64,
        unpacked_size_bytes: u64,
        checksum_sha256: &str,
        runtime_kind: &str,
        output_api_version: &str,
        framework_name: Option<&str>,
        framework_version: Option<&str>,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        let file = tokio::fs::File::open(artifact_path)
            .await
            .with_context(|| format!("failed to open artifact {}", artifact_path.display()))?;
        let actual_size = file
            .metadata()
            .await
            .context("failed to read artifact metadata")?
            .len();
        if actual_size != packed_size_bytes {
            anyhow::bail!(
                "artifact size changed before upload: expected {packed_size_bytes}, found {actual_size}"
            );
        }
        let mut request = self
            .http
            .put(self.url(&format!("/deployments/{deployment_id}/static-site")))
            .bearer_auth(&self.token)
            .header(artifact_headers::RUNTIME_KIND, runtime_kind)
            .header(artifact_headers::OUTPUT_API_VERSION, output_api_version)
            .header(artifact_headers::CHECKSUM_SHA256, checksum_sha256)
            .header(artifact_headers::PACKED_SIZE_BYTES, packed_size_bytes)
            .header(artifact_headers::UNPACKED_SIZE_BYTES, unpacked_size_bytes)
            .timeout(Duration::from_secs(600))
            .body(reqwest::Body::wrap_stream(ReaderStream::new(file)));
        if let Some(name) = framework_name {
            request = request.header(artifact_headers::FRAMEWORK_NAME, name);
        }
        if let Some(version) = framework_version {
            request = request.header(artifact_headers::FRAMEWORK_VERSION, version);
        }
        Ok(request)
    }

    #[allow(dead_code)] // Wired by the build loop in Milestone 7.
    #[allow(clippy::too_many_arguments)]
    pub async fn upload_artifact(
        &self,
        deployment_id: Uuid,
        artifact_path: &Path,
        packed_size_bytes: u64,
        unpacked_size_bytes: u64,
        checksum_sha256: &str,
        runtime_kind: &str,
        output_api_version: &str,
        framework_name: Option<&str>,
        framework_version: Option<&str>,
    ) -> anyhow::Result<UploadArtifactResponse> {
        let response = self
            .artifact_upload_request(
                deployment_id,
                artifact_path,
                packed_size_bytes,
                unpacked_size_bytes,
                checksum_sha256,
                runtime_kind,
                output_api_version,
                framework_name,
                framework_version,
            )
            .await?
            .send()
            .await
            .context("deployment.upload_artifact: request failed")?;
        Self::unwrap_envelope(response, "deployment.upload_artifact").await
    }

    pub async fn download_artifact_to(
        &self,
        assignment: &ServeAssignment,
        destination: &Path,
    ) -> anyhow::Result<()> {
        let response = self
            .http
            .get(self.url(&format!(
                "/deployments/{}/artifact",
                assignment.deployment_id
            )))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .context("deployment.download_artifact: request failed")?;
        if !response.status().is_success() {
            anyhow::bail!(
                "deployment.download_artifact: control api returned {}",
                response.status()
            );
        }
        let header = |name: &'static str| -> anyhow::Result<&str> {
            response
                .headers()
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("artifact response missing {name} header"))?
                .to_str()
                .with_context(|| format!("artifact response has invalid {name} header"))
        };
        let packed_size = header(artifact_headers::PACKED_SIZE_BYTES)?
            .parse::<u64>()
            .context("artifact response has invalid packed size")?;
        let unpacked_size = header(artifact_headers::UNPACKED_SIZE_BYTES)?
            .parse::<u64>()
            .context("artifact response has invalid unpacked size")?;
        let checksum = header(artifact_headers::CHECKSUM_SHA256)?.to_owned();
        if packed_size != assignment.artifact.packed_size_bytes
            || unpacked_size != assignment.artifact.unpacked_size_bytes
            || checksum != assignment.artifact.checksum_sha256
        {
            anyhow::bail!("artifact response metadata does not match assignment");
        }

        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .await?;
        let mut stream = response.bytes_stream();
        let result = async {
            let mut actual_size = 0_u64;
            let mut hasher = Sha256::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.context("artifact response body failed")?;
                actual_size = actual_size
                    .checked_add(chunk.len() as u64)
                    .ok_or_else(|| anyhow::anyhow!("artifact response is too large"))?;
                if actual_size > packed_size {
                    anyhow::bail!("artifact response exceeds its packed size");
                }
                file.write_all(&chunk).await?;
                hasher.update(&chunk);
            }
            file.flush().await?;
            file.sync_all().await?;
            if actual_size != packed_size {
                anyhow::bail!(
                    "artifact response size mismatch: expected {packed_size}, found {actual_size}"
                );
            }
            let actual_checksum = hex::encode(hasher.finalize());
            if actual_checksum != checksum {
                anyhow::bail!(
                    "artifact response checksum mismatch: expected {checksum}, found {actual_checksum}"
                );
            }
            Ok(())
        }
        .await;
        drop(file);
        if result.is_err() {
            let _ = tokio::fs::remove_file(destination).await;
        }
        result
    }

    pub async fn serve_assignments(&self) -> anyhow::Result<ServeAssignmentsResponse> {
        let response = self
            .http
            .get(self.url("/serve/assignments"))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("serve.assignments: request failed")?;
        Self::unwrap_envelope(response, "serve.assignments").await
    }

    pub async fn report_serve_status(
        &self,
        deployment_id: Uuid,
        request: &ReportServeStatusRequest,
    ) -> anyhow::Result<ReportServeStatusResponse> {
        self.post_json(
            &format!("/serve/deployments/{deployment_id}/status"),
            request,
            "serve.report_status",
        )
        .await
    }

    pub async fn route_snapshot(&self) -> anyhow::Result<RouteSnapshotResponse> {
        let response = self
            .http
            .get(self.url("/serve/routes"))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("serve.routes: request failed")?;
        Self::unwrap_envelope(response, "serve.routes").await
    }

    #[allow(dead_code)] // Wired by the serve resolver in Milestone 10.
    pub async fn resolve_host(&self, host: &str) -> anyhow::Result<Option<ResolveHostResponse>> {
        let response = self
            .http
            .get(self.url("/serve/resolve-host"))
            .query(&[("host", host)])
            .bearer_auth(&self.token)
            .send()
            .await
            .context("serve.resolve_host: request failed")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Self::unwrap_envelope(response, "serve.resolve_host")
            .await
            .map(Some)
    }

    pub async fn start_preview_authorization(
        &self,
        host: &str,
        return_to: &str,
    ) -> anyhow::Result<StartPreviewAuthorizationResponse> {
        self.post_json(
            "/serve/preview/authorize",
            &StartPreviewAuthorizationRequest {
                host: host.to_owned(),
                return_to: return_to.to_owned(),
            },
            "serve.preview_authorize",
        )
        .await
    }

    pub async fn exchange_preview_code(
        &self,
        host: &str,
        code: &str,
    ) -> Result<ExchangePreviewCodeResponse, PreviewAuthError> {
        self.post_preview_json(
            "/serve/preview/exchange",
            &ExchangePreviewCodeRequest {
                host: host.to_owned(),
                code: code.to_owned(),
            },
            "serve.preview_exchange",
        )
        .await
    }

    pub async fn verify_preview_grant(
        &self,
        host: &str,
        grant: &str,
    ) -> Result<VerifyPreviewGrantResponse, PreviewAuthError> {
        self.post_preview_json(
            "/serve/preview/verify",
            &VerifyPreviewGrantRequest {
                host: host.to_owned(),
                grant: grant.to_owned(),
            },
            "serve.preview_verify",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, body::Body, response::Response, routing::get};
    use grass_node_protocol::{
        ServeArtifact, ServeAssignment, ServeAssignmentStatus, ServeResources,
    };
    use sha2::{Digest, Sha256};

    use super::*;

    #[tokio::test]
    async fn artifact_upload_request_streams_file_and_sets_size_headers() {
        let path = std::env::temp_dir().join(format!("grass-upload-{}.zip", Uuid::now_v7()));
        tokio::fs::write(&path, b"zip-bytes").await.unwrap();
        let client = ControlApiClient::new("http://127.0.0.1:9", "node-token").unwrap();

        let request = client
            .artifact_upload_request(
                Uuid::nil(),
                &path,
                9,
                1025,
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "static",
                "1",
                Some("vite"),
                None,
            )
            .await
            .unwrap()
            .build()
            .unwrap();

        assert!(request.body().unwrap().as_bytes().is_none());
        assert_eq!(request.headers()[artifact_headers::PACKED_SIZE_BYTES], "9");
        assert_eq!(
            request.headers()[artifact_headers::UNPACKED_SIZE_BYTES],
            "1025"
        );
        assert_eq!(
            request.headers()[artifact_headers::CHECKSUM_SHA256],
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn artifact_download_streams_and_verifies_metadata() {
        let body = b"zip-bytes";
        let checksum = hex::encode(Sha256::digest(body));
        let assignment = ServeAssignment {
            deployment_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            runtime_kind: "static".to_owned(),
            status: ServeAssignmentStatus::Pending,
            artifact: ServeArtifact {
                artifact_id: Uuid::now_v7(),
                checksum_sha256: checksum.clone(),
                packed_size_bytes: body.len() as u64,
                unpacked_size_bytes: 99,
            },
            resources: ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 256,
            },
        };
        let response_checksum = checksum.clone();
        let app = Router::new().route(
            "/api/v1/internal/deployments/{deployment_id}/artifact",
            get(move || {
                let checksum = response_checksum.clone();
                async move {
                    Response::builder()
                        .header(artifact_headers::PACKED_SIZE_BYTES, "9")
                        .header(artifact_headers::UNPACKED_SIZE_BYTES, "99")
                        .header(artifact_headers::CHECKSUM_SHA256, checksum)
                        .body(Body::from(body.as_slice()))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();
        let destination =
            std::env::temp_dir().join(format!("grass-download-{}.zip", Uuid::now_v7().simple()));

        client
            .download_artifact_to(&assignment, &destination)
            .await
            .unwrap();

        assert_eq!(tokio::fs::read(&destination).await.unwrap(), body);
        tokio::fs::remove_file(destination).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn serve_assignment_and_status_calls_use_internal_protocol() {
        use grass_node_protocol::{
            ReportServeStatusRequest, ReportServeStatusResponse, ReportedServeStatus,
            RouteSnapshotResponse, ServeAssignmentsResponse,
        };

        let deployment_id = Uuid::now_v7();
        let app = Router::new()
            .route(
                "/api/v1/internal/serve/assignments",
                get(|| async {
                    axum::Json(serde_json::json!({
                        "code": 200,
                        "message": "OK",
                        "data": { "assignments": [] }
                    }))
                }),
            )
            .route(
                "/api/v1/internal/serve/deployments/{deployment_id}/status",
                axum::routing::post(
                    |axum::Json(report): axum::Json<ReportServeStatusRequest>| async move {
                        assert_eq!(report.status, ReportedServeStatus::Syncing);
                        axum::Json(serde_json::json!({
                            "code": 200,
                            "message": "OK",
                            "data": { "acknowledged": true }
                        }))
                    },
                ),
            )
            .route(
                "/api/v1/internal/serve/routes",
                get(|| async {
                    axum::Json(serde_json::json!({
                        "code": 200,
                        "message": "OK",
                        "data": { "revision": "abc123", "routes": [] }
                    }))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();

        let assignments: ServeAssignmentsResponse = client.serve_assignments().await.unwrap();
        let status: ReportServeStatusResponse = client
            .report_serve_status(
                deployment_id,
                &ReportServeStatusRequest {
                    status: ReportedServeStatus::Syncing,
                    failure_code: None,
                    failure_message: None,
                },
            )
            .await
            .unwrap();
        let routes: RouteSnapshotResponse = client.route_snapshot().await.unwrap();

        assert!(assignments.assignments.is_empty());
        assert!(status.acknowledged);
        assert_eq!(routes.revision, "abc123");
        assert!(routes.routes.is_empty());
        server.abort();
    }
}
