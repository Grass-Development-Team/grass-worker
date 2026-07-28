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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub acknowledged: bool,
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
pub struct ServeRoute {
    pub host: String,
    pub deployment_id: Uuid,
    pub target_node_id: Uuid,
    pub target_base_url: String,
    pub resources: ServeResources,
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
    pub environment: String,
    /// Whether a grass-output artifact upload finished for this deployment.
    pub artifact_available: bool,
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
        };
        let parsed: ServeRoute =
            serde_json::from_slice(&serde_json::to_vec(&route).unwrap()).unwrap();
        assert_eq!(parsed.host, "app.example.com");
        assert_eq!(parsed.resources, resources);
    }
}
