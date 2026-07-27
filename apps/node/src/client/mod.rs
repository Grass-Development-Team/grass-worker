//! HTTP client for the Control API internal protocol.

use std::{path::Path, time::Duration};

use anyhow::Context;
use grass_node_protocol::{
    AppendBuildLogRequest, AppendBuildLogResponse, ClaimRequest, ClaimResponse, HeartbeatRequest,
    HeartbeatResponse, ObserveSshHostKeyRequest, ObserveSshHostKeyResponse,
    RedeemGitCredentialRequest, RedeemGitCredentialResponse, RegisterRequest, RegisterResponse,
    ResolveHostResponse, StageRequest, StageResponse, UploadArtifactResponse, artifact_headers,
};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

/// Control API response envelope.
#[derive(Debug, Deserialize)]
struct Envelope<T> {
    code: u16,
    message: String,
    data: Option<T>,
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

    /// Downloads the grass-output archive for serving; `None` when the
    /// artifact does not exist.
    #[allow(dead_code)] // Wired by the serve resolver in Milestone 10.
    pub async fn download_artifact(&self, deployment_id: Uuid) -> anyhow::Result<Option<Vec<u8>>> {
        let response = self
            .http
            .get(self.url(&format!("/deployments/{deployment_id}/artifact")))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .context("deployment.download_artifact: request failed")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            anyhow::bail!(
                "deployment.download_artifact: control api returned {}",
                response.status()
            );
        }
        let bytes = response
            .bytes()
            .await
            .context("deployment.download_artifact: failed to read body")?;
        Ok(Some(bytes.to_vec()))
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
}

#[cfg(test)]
mod tests {
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
}
