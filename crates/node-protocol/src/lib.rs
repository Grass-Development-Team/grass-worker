//! Shared request/response types for the Control API ↔ Node internal
//! protocol. Both sides depend on this crate so the wire format can only
//! change in one place.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable stage identifiers reported during a build. The Console groups log
/// lines and progress by these values.
pub mod stage {
    pub const QUEUED: &str = "queued";
    pub const CHECKOUT: &str = "checkout";
    pub const INSTALL: &str = "install";
    pub const BUILD: &str = "build";
    pub const OUTPUT: &str = "output";
    pub const ARCHIVE: &str = "archive";
    pub const UPLOAD: &str = "upload";
}

// --- Registration -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub build: bool,
    pub serve: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeResources {
    pub cpu_millicores: u64,
    pub memory_mb: u64,
    pub disk_mb: u64,
    pub max_deployments: u32,
}

/// Complete Node configuration that is safe to synchronize through the
/// Control API. Authentication tokens are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfiguration {
    pub node: NodeIdentityConfiguration,
    pub build: NodeBuildConfiguration,
    pub serve: NodeServeConfiguration,
    pub runtime: NodeRuntimeConfiguration,
    pub security: NodeSecurityConfiguration,
    pub development: NodeDevelopmentConfiguration,
    pub log: NodeLogConfiguration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentityConfiguration {
    pub id: String,
    pub control_api: String,
    pub work_root: String,
    pub capabilities: NodeCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeBuildConfiguration {
    pub concurrency: u16,
    pub command_timeout_seconds: u64,
    pub retain_workspace_on_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeServeConfiguration {
    pub host: String,
    pub port: u16,
    pub public_base_url: String,
    pub metadata_cache_ttl_seconds: u64,
    pub artifact_cache_root: String,
    pub capacity: NodeServeCapacityConfiguration,
    pub ssr: NodeSsrConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeServeCapacityConfiguration {
    pub cpu_millicores: u64,
    pub memory_mb: u64,
    pub disk_mb: u64,
    pub max_deployments: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSsrConfiguration {
    pub idle_stop_seconds: u64,
    pub startup_timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRuntimeConfiguration {
    pub backend: String,
    pub socket: String,
    pub default_build_image: String,
    pub default_serve_image: String,
    pub network: String,
    pub resources: NodeRuntimeResourcesConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRuntimeResourcesConfiguration {
    pub cpu_limit: u32,
    pub memory_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSecurityConfiguration {
    pub private_repository_targets: Vec<NodePrivateRepositoryTargetConfiguration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePrivateRepositoryTargetConfiguration {
    pub host: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDevelopmentConfiguration {
    pub verbose_build_log: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeLogConfiguration {
    pub level: String,
    pub format: NodeLogFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLogFormat {
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServeResources {
    pub cpu_millicores: u64,
    pub memory_mb: u64,
    pub disk_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub version: String,
    pub capabilities: NodeCapabilities,
    pub build_concurrency: u16,
    /// Public base URL of the Node serve listener, when known.
    #[serde(default)]
    pub serve_base_url: Option<String>,
    /// Schedulable Serve capacity. Build-only Nodes omit this field.
    #[serde(default)]
    pub resources: Option<NodeResources>,
    /// Revision loaded by the running Node process.
    #[serde(default)]
    pub config_revision: u64,
    /// Effective non-secret configuration loaded by this process.
    #[serde(default)]
    pub effective_config: Option<NodeConfiguration>,
    /// Whether the running process has a usable token, without exposing it.
    #[serde(default)]
    pub node_token_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub node_id: Uuid,
    pub name: String,
    /// Capabilities after server-side correction; the first stage forces
    /// build and serve on.
    pub capabilities: NodeCapabilities,
    /// Shared credential for authenticated Serve-to-Serve proxying. It is
    /// present only when Serve capability is enabled.
    #[serde(default)]
    pub gateway_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    /// Number of builds currently running on the Node.
    #[serde(default)]
    pub active_builds: u16,
    /// Revision currently used by the running process.
    #[serde(default)]
    pub effective_config_revision: u64,
    /// Desired revision written to disk and awaiting process restart.
    #[serde(default)]
    pub applying_config_revision: Option<u64>,
    /// Last failure while persisting the desired configuration.
    #[serde(default)]
    pub config_apply_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub acknowledged: bool,
    /// Latest desired revision, when it differs from the running process.
    #[serde(default)]
    pub desired_config_revision: Option<u64>,
    /// Complete desired non-secret configuration for the Node to persist.
    #[serde(default)]
    pub desired_config: Option<NodeConfiguration>,
}

// --- Claim ------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRequest {
    /// How many additional builds the Node can take right now.
    pub capacity: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClaimResponse {
    #[serde(default)]
    pub deployment: Option<ClaimedDeployment>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClaimedDeployment {
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub team_id: Uuid,
    pub environment: String,
    pub runtime_kind: String,
    pub repository_url: String,
    pub branch: Option<String>,
    pub commit_hash: Option<String>,
    pub root_directory: Option<String>,
    pub install_command: Option<String>,
    pub build_command: Option<String>,
    pub output_directory: Option<String>,
    /// Per-build timeout resolved from the team quota plan; `None` means no
    /// limit is configured.
    pub build_timeout_seconds: Option<i64>,
    /// Preview host assigned at creation time, if any.
    pub preview_host: Option<String>,
    /// Opaque, short-lived, one-time token. The Node exchanges it separately
    /// so credential material never appears in deployment snapshots.
    #[serde(default)]
    pub source_credential_lease: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RedeemGitCredentialRequest {
    pub lease: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitCredential {
    Https {
        username: String,
        secret: String,
    },
    Ssh {
        username: String,
        private_key: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RedeemGitCredentialResponse {
    pub credential: GitCredential,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ObserveSshHostKeyRequest {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ObserveSshHostKeyResponse {
    pub approved: bool,
    #[serde(default)]
    pub known_hosts_line: Option<String>,
}

// --- Stage reports ----------------------------------------------------------

/// A build status/stage report from the Node. `status: None` reports a stage
/// change inside the current status (for example install → build while
/// `building`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRequest {
    #[serde(default)]
    pub status: Option<ReportedStatus>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_message: Option<String>,
    /// Whole build minutes consumed, reported once with the terminal status.
    #[serde(default)]
    pub build_minutes: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportedStatus {
    Queued,
    Building,
    Ready,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResponse {
    /// The server asks the Node to stop this build (user cancel).
    pub cancel_requested: bool,
}

// --- Build log --------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildLogLine {
    pub seq: u64,
    pub stage: String,
    pub line: String,
    /// Unix timestamp in milliseconds.
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendBuildLogRequest {
    pub lines: Vec<BuildLogLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendBuildLogResponse {
    pub last_seq: u64,
}

// --- Artifact upload --------------------------------------------------------

/// Metadata travelling with a `grass-output.zip` upload as HTTP headers.
pub mod artifact_headers {
    pub const RUNTIME_KIND: &str = "x-grass-runtime-kind";
    pub const OUTPUT_API_VERSION: &str = "x-grass-output-api-version";
    pub const FRAMEWORK_NAME: &str = "x-grass-framework-name";
    pub const FRAMEWORK_VERSION: &str = "x-grass-framework-version";
    pub const CHECKSUM_SHA256: &str = "x-grass-checksum-sha256";
    pub const PACKED_SIZE_BYTES: &str = "x-grass-packed-size-bytes";
    pub const UNPACKED_SIZE_BYTES: &str = "x-grass-unpacked-size-bytes";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadArtifactResponse {
    pub artifact_id: Uuid,
    pub size_bytes: i64,
    pub checksum_sha256: String,
}

// --- Serve resolution -------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServeAccess {
    Public,
    TeamOrPlatformAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeArtifact {
    pub artifact_id: Uuid,
    pub checksum_sha256: String,
    pub packed_size_bytes: u64,
    pub unpacked_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeAssignment {
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub runtime_kind: String,
    pub status: ServeAssignmentStatus,
    pub artifact: ServeArtifact,
    pub resources: ServeResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServeAssignmentStatus {
    Pending,
    Syncing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeAssignmentsResponse {
    pub assignments: Vec<ServeAssignment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportedServeStatus {
    Syncing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportServeStatusRequest {
    pub status: ReportedServeStatus,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportServeStatusResponse {
    pub acknowledged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsrLeaseResponse {
    pub lease_id: Uuid,
    pub expires_at_unix: i64,
    pub hour_block_start_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeRoute {
    pub host: String,
    pub deployment_id: Uuid,
    pub target_node_id: Uuid,
    pub target_base_url: String,
    pub resources: ServeResources,
    pub access: ServeAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSnapshotResponse {
    pub revision: String,
    pub routes: Vec<ServeRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveHostResponse {
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub team_id: Uuid,
    pub host: String,
    pub environment: String,
    /// Whether a grass-output artifact upload finished for this deployment.
    pub artifact_available: bool,
    pub access: ServeAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPreviewAuthorizationRequest {
    pub host: String,
    pub return_to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPreviewAuthorizationResponse {
    pub authorization_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangePreviewCodeRequest {
    pub host: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangePreviewCodeResponse {
    pub grant: String,
    pub return_to: String,
    pub max_age_seconds: u64,
    #[serde(default = "secure_cookie_by_default")]
    pub cookie_secure: bool,
}

fn secure_cookie_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyPreviewGrantRequest {
    pub host: String,
    pub grant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyPreviewGrantResponse {
    pub allowed: bool,
}

// --- Realtime log stream (Node → Control API → Browser) ---------------------

/// JSON messages shared by the Node→API and API→Browser websocket legs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LogStreamMessage {
    /// One log line (server → client, Node → API).
    Log {
        deployment_id: Uuid,
        stage: String,
        line: String,
        timestamp_ms: i64,
        seq: u64,
    },
    /// The build moved to a new stage.
    StageChange { deployment_id: Uuid, stage: String },
    /// The build reached a terminal status.
    Done {
        deployment_id: Uuid,
        build_status: String,
    },
    /// Browser → API: start receiving messages for this deployment.
    Subscribe { deployment_id: Uuid },
    /// Browser → API → Node: stop this build.
    Cancel { deployment_id: Uuid },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node_configuration_json() -> serde_json::Value {
        serde_json::json!({
            "node": {
                "id": "node-a",
                "control_api": "https://control.example.test",
                "work_root": "/data/node",
                "capabilities": { "build": true, "serve": true }
            },
            "build": {
                "concurrency": 2,
                "command_timeout_seconds": 600,
                "retain_workspace_on_failure": false
            },
            "serve": {
                "host": "0.0.0.0",
                "port": 8080,
                "public_base_url": "https://node-a.example.test",
                "metadata_cache_ttl_seconds": 30,
                "artifact_cache_root": "/data/node/artifacts",
                "capacity": {
                    "cpu_millicores": 2_000,
                    "memory_mb": 4_096,
                    "disk_mb": 20_480,
                    "max_deployments": 20
                },
                "ssr": { "idle_stop_seconds": 1_800, "startup_timeout_seconds": 90 }
            },
            "runtime": {
                "backend": "podman-socket",
                "socket": "unix:///run/user/1000/podman/podman.sock",
                "default_build_image": "docker.io/library/node:22",
                "default_serve_image": "docker.io/library/node:22",
                "network": "bridge",
                "resources": { "cpu_limit": 2, "memory_mb": 2_048 }
            },
            "security": {
                "private_repository_targets": [
                    { "host": "git.internal.example", "ip": "10.0.0.8", "port": 2222 }
                ]
            },
            "development": { "verbose_build_log": true },
            "log": { "level": "info", "format": "pretty" }
        })
    }

    #[test]
    fn node_configuration_sync_fields_round_trip_without_secrets() {
        let registration: RegisterRequest = serde_json::from_value(serde_json::json!({
            "name": "node-a",
            "version": "0.1.0",
            "capabilities": { "build": true, "serve": true },
            "build_concurrency": 2,
            "serve_base_url": "https://node-a.example.test",
            "resources": {
                "cpu_millicores": 2_000,
                "memory_mb": 4_096,
                "disk_mb": 20_480,
                "max_deployments": 20
            },
            "config_revision": 7,
            "effective_config": node_configuration_json(),
            "node_token_configured": true
        }))
        .unwrap();
        let registration = serde_json::to_value(registration).unwrap();
        assert_eq!(registration["config_revision"], 7);
        assert_eq!(
            registration["effective_config"]["runtime"]["default_serve_image"],
            "docker.io/library/node:22"
        );
        assert_eq!(registration["node_token_configured"], true);
        assert!(registration.to_string().find("node_token\"").is_none());

        let heartbeat: HeartbeatRequest = serde_json::from_value(serde_json::json!({
            "active_builds": 1,
            "effective_config_revision": 6,
            "applying_config_revision": 7,
            "config_apply_error": null
        }))
        .unwrap();
        let heartbeat = serde_json::to_value(heartbeat).unwrap();
        assert_eq!(heartbeat["effective_config_revision"], 6);
        assert_eq!(heartbeat["applying_config_revision"], 7);

        let response: HeartbeatResponse = serde_json::from_value(serde_json::json!({
            "acknowledged": true,
            "desired_config_revision": 8,
            "desired_config": node_configuration_json()
        }))
        .unwrap();
        let response = serde_json::to_value(response).unwrap();
        assert_eq!(response["desired_config_revision"], 8);
        assert_eq!(response["desired_config"]["node"]["id"], "node-a");
    }

    #[test]
    fn log_stream_messages_round_trip_as_tagged_json() {
        let message = LogStreamMessage::Log {
            deployment_id: Uuid::nil(),
            stage: stage::BUILD.to_owned(),
            line: "compiled".to_owned(),
            timestamp_ms: 1_000,
            seq: 42,
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains(r#""type":"log""#));
        let parsed: LogStreamMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            LogStreamMessage::Log { seq, .. } => assert_eq!(seq, 42),
            _ => panic!("wrong variant"),
        }

        let subscribe: LogStreamMessage = serde_json::from_str(
            r#"{"type":"subscribe","deployment_id":"00000000-0000-0000-0000-000000000000"}"#,
        )
        .unwrap();
        assert!(matches!(subscribe, LogStreamMessage::Subscribe { .. }));
    }

    #[test]
    fn stage_request_supports_stage_only_updates() {
        let request: StageRequest = serde_json::from_str(r#"{"stage":"install"}"#).unwrap();
        assert!(request.status.is_none());
        assert_eq!(request.stage.as_deref(), Some("install"));

        let terminal: StageRequest = serde_json::from_str(
            r#"{"status":"failed","failure_code":"build_failed","failure_message":"exit 1","build_minutes":3}"#,
        )
        .unwrap();
        assert_eq!(terminal.status, Some(ReportedStatus::Failed));
        assert_eq!(terminal.build_minutes, Some(3));
    }

    #[test]
    fn preview_access_protocol_uses_stable_wire_values() {
        assert_eq!(
            serde_json::to_string(&ServeAccess::TeamOrPlatformAdmin).unwrap(),
            r#""team_or_platform_admin""#
        );

        let start: StartPreviewAuthorizationRequest =
            serde_json::from_str(r#"{"host":"demo.example.test","return_to":"/docs?q=1"}"#)
                .unwrap();
        assert_eq!(start.host, "demo.example.test");
        assert_eq!(start.return_to, "/docs?q=1");

        let exchange: ExchangePreviewCodeResponse = serde_json::from_str(
            r#"{"grant":"opaque","return_to":"/docs?q=1","max_age_seconds":43200,"cookie_secure":false}"#,
        )
        .unwrap();
        assert_eq!(exchange.grant, "opaque");
        assert_eq!(exchange.max_age_seconds, 43_200);
        assert!(!exchange.cookie_secure);
        let legacy_exchange: ExchangePreviewCodeResponse =
            serde_json::from_str(r#"{"grant":"opaque","return_to":"/","max_age_seconds":60}"#)
                .unwrap();
        assert!(legacy_exchange.cookie_secure);

        let verify = VerifyPreviewGrantResponse { allowed: true };
        assert_eq!(
            serde_json::to_string(&verify).unwrap(),
            r#"{"allowed":true}"#
        );
    }

    #[test]
    fn serve_protocol_round_trips_resources_status_and_routes() {
        let resources = ServeResources {
            cpu_millicores: 200,
            memory_mb: 256,
            disk_mb: 512,
        };
        assert_eq!(
            serde_json::to_value(resources).unwrap()["cpu_millicores"],
            200
        );

        let status: ReportedServeStatus = serde_json::from_str("\"syncing\"").unwrap();
        assert_eq!(status, ReportedServeStatus::Syncing);
        let assignment_status: ServeAssignmentStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(assignment_status, ServeAssignmentStatus::Pending);

        let route = ServeRoute {
            host: "app.example.com".to_owned(),
            deployment_id: Uuid::nil(),
            target_node_id: Uuid::nil(),
            target_base_url: "http://node-1:8080".to_owned(),
            resources,
            access: ServeAccess::TeamOrPlatformAdmin,
        };
        let parsed: ServeRoute =
            serde_json::from_slice(&serde_json::to_vec(&route).unwrap()).unwrap();
        assert_eq!(parsed.host, "app.example.com");
        assert_eq!(parsed.resources, resources);
        assert_eq!(parsed.access, ServeAccess::TeamOrPlatformAdmin);
    }
}
