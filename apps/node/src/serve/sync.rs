use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::StreamExt;
use grass_node_protocol::{
    ReportServeStatusRequest, ReportedServeStatus, ServeAssignment, ServeAssignmentStatus,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::output::manifest;

const MARKER_FILE: &str = ".artifact.json";
const SYNC_INTERVAL: Duration = Duration::from_secs(5);
const MAX_CONCURRENT_SYNCS: usize = 4;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ArtifactMarker {
    deployment_id: Uuid,
    artifact_id: Uuid,
    checksum_sha256: String,
    packed_size_bytes: u64,
    unpacked_size_bytes: u64,
}

impl ArtifactMarker {
    fn from_assignment(assignment: &ServeAssignment) -> Self {
        Self {
            deployment_id: assignment.deployment_id,
            artifact_id: assignment.artifact.artifact_id,
            checksum_sha256: assignment.artifact.checksum_sha256.clone(),
            packed_size_bytes: assignment.artifact.packed_size_bytes,
            unpacked_size_bytes: assignment.artifact.unpacked_size_bytes,
        }
    }
}

fn verify_archive_file(assignment: &ServeAssignment, archive_path: &Path) -> anyhow::Result<()> {
    let mut file = File::open(archive_path)?;
    let size = file.metadata()?.len();
    if size != assignment.artifact.packed_size_bytes {
        anyhow::bail!(
            "artifact packed size mismatch: expected {}, found {size}",
            assignment.artifact.packed_size_bytes
        );
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let checksum = hex::encode(hasher.finalize());
    if checksum != assignment.artifact.checksum_sha256 {
        anyhow::bail!(
            "artifact checksum mismatch: expected {}, found {checksum}",
            assignment.artifact.checksum_sha256
        );
    }
    Ok(())
}

fn marker_matches(path: &Path, expected: &ArtifactMarker) -> bool {
    std::fs::read(path.join(MARKER_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ArtifactMarker>(&bytes).ok())
        .is_some_and(|marker| marker == *expected)
}

pub fn staged_artifact_path(cache_root: &Path, deployment_id: Uuid) -> anyhow::Result<PathBuf> {
    let deployment_root = cache_root.join(deployment_id.to_string());
    for entry in match std::fs::read_dir(&deployment_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("artifact is not staged locally")
        }
        Err(error) => return Err(error.into()),
    } {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let marker = std::fs::read(path.join(MARKER_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ArtifactMarker>(&bytes).ok());
        if marker.is_some_and(|marker| marker.deployment_id == deployment_id) {
            return Ok(path);
        }
    }
    anyhow::bail!("artifact is not staged locally")
}

pub fn stage_archive(
    cache_root: &Path,
    assignment: &ServeAssignment,
    archive_path: &Path,
) -> anyhow::Result<PathBuf> {
    let deployment_root = cache_root.join(assignment.deployment_id.to_string());
    let final_path = deployment_root.join(&assignment.artifact.checksum_sha256);
    let marker = ArtifactMarker::from_assignment(assignment);
    if marker_matches(&final_path, &marker) {
        return Ok(final_path);
    }

    std::fs::create_dir_all(&deployment_root)?;
    let staging_path = deployment_root.join(format!(".staging-{}", Uuid::now_v7().simple()));
    std::fs::create_dir(&staging_path)?;

    let result = (|| {
        verify_archive_file(assignment, archive_path)?;
        let unpacked = grass_archive::unpack_zip(archive_path, &staging_path)?;
        if assignment.artifact.unpacked_size_bytes != 0
            && unpacked.unpacked_size_bytes != assignment.artifact.unpacked_size_bytes
        {
            anyhow::bail!(
                "artifact unpacked size mismatch: expected {}, found {}",
                assignment.artifact.unpacked_size_bytes,
                unpacked.unpacked_size_bytes
            );
        }

        let manifest_content = std::fs::read_to_string(staging_path.join("output.toml"))?;
        let output = manifest::parse_manifest(&manifest_content)
            .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;
        manifest::validate_manifest(&output, &staging_path)
            .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;
        if output.runtime.kind != assignment.runtime_kind {
            anyhow::bail!(
                "artifact runtime mismatch: expected {}, found {}",
                assignment.runtime_kind,
                output.runtime.kind
            );
        }

        std::fs::write(staging_path.join(MARKER_FILE), serde_json::to_vec(&marker)?)?;
        if final_path.exists() {
            std::fs::remove_dir_all(&final_path)?;
        }
        std::fs::rename(&staging_path, &final_path)?;
        Ok(final_path.clone())
    })();

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging_path);
    }
    result
}

async fn report_status(
    client: &crate::client::ControlApiClient,
    deployment_id: Uuid,
    status: ReportedServeStatus,
    failure: Option<(&str, String)>,
) -> anyhow::Result<()> {
    let (failure_code, failure_message) = failure
        .map(|(code, message)| (Some(code.to_owned()), Some(message)))
        .unwrap_or_default();
    client
        .report_serve_status(
            deployment_id,
            &ReportServeStatusRequest {
                status,
                failure_code,
                failure_message,
            },
        )
        .await?;
    Ok(())
}

fn bounded_failure_message(error: &anyhow::Error) -> String {
    let message = format!("{error:#}");
    let mut end = message.len().min(1024);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message[..end].to_owned()
}

async fn cleanup_ephemeral(deployment_root: &Path) -> anyhow::Result<()> {
    let mut entries = tokio::fs::read_dir(deployment_root).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".download-") && entry.file_type().await?.is_file() {
            tokio::fs::remove_file(entry.path()).await?;
        } else if name.starts_with(".staging-") && entry.file_type().await?.is_dir() {
            tokio::fs::remove_dir_all(entry.path()).await?;
        }
    }
    Ok(())
}

pub async fn sync_assignment(
    client: &crate::client::ControlApiClient,
    cache_root: &Path,
    assignment: ServeAssignment,
) -> anyhow::Result<()> {
    let deployment_root = cache_root.join(assignment.deployment_id.to_string());
    tokio::fs::create_dir_all(&deployment_root).await?;
    cleanup_ephemeral(&deployment_root).await?;
    let final_path = deployment_root.join(&assignment.artifact.checksum_sha256);
    if marker_matches(&final_path, &ArtifactMarker::from_assignment(&assignment)) {
        if matches!(
            assignment.status,
            ServeAssignmentStatus::Pending | ServeAssignmentStatus::Failed
        ) {
            report_status(
                client,
                assignment.deployment_id,
                ReportedServeStatus::Syncing,
                None,
            )
            .await?;
        }
        if !matches!(assignment.status, ServeAssignmentStatus::Ready) {
            report_status(
                client,
                assignment.deployment_id,
                ReportedServeStatus::Ready,
                None,
            )
            .await?;
        }
        return Ok(());
    }

    if !matches!(assignment.status, ServeAssignmentStatus::Syncing) {
        report_status(
            client,
            assignment.deployment_id,
            ReportedServeStatus::Syncing,
            None,
        )
        .await?;
    }

    let download_path = deployment_root.join(format!(".download-{}.zip", Uuid::now_v7().simple()));
    let sync_result = async {
        client
            .download_artifact_to(&assignment, &download_path)
            .await?;
        let cache_root = cache_root.to_owned();
        let staged_assignment = assignment.clone();
        let staged_download = download_path.clone();
        tokio::task::spawn_blocking(move || {
            stage_archive(&cache_root, &staged_assignment, &staged_download)
        })
        .await??;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let _ = tokio::fs::remove_file(&download_path).await;

    match sync_result {
        Ok(()) => {
            report_status(
                client,
                assignment.deployment_id,
                ReportedServeStatus::Ready,
                None,
            )
            .await
        }
        Err(error) => {
            let message = bounded_failure_message(&error);
            if let Err(report_error) = report_status(
                client,
                assignment.deployment_id,
                ReportedServeStatus::Failed,
                Some(("artifact_sync_failed", message)),
            )
            .await
            {
                tracing::warn!(
                    operation = "node.serve.sync.report_failed",
                    deployment_id = %assignment.deployment_id,
                    error = %report_error,
                    "failed to report artifact sync failure"
                );
            }
            Err(error)
        }
    }
}

fn unique_assignments(assignments: Vec<ServeAssignment>) -> Vec<ServeAssignment> {
    let mut seen = HashSet::with_capacity(assignments.len());
    assignments
        .into_iter()
        .filter(|assignment| seen.insert(assignment.deployment_id))
        .collect()
}

pub fn spawn(
    client: crate::client::ControlApiClient,
    cache_root: PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SYNC_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let assignments = match client.serve_assignments().await {
                Ok(response) => unique_assignments(response.assignments),
                Err(error) => {
                    tracing::warn!(
                        operation = "node.serve.sync.assignments",
                        %error,
                        "failed to fetch Serve assignments"
                    );
                    continue;
                }
            };
            futures_util::stream::iter(assignments)
                .for_each_concurrent(MAX_CONCURRENT_SYNCS, |assignment| {
                    let client = client.clone();
                    let cache_root = cache_root.clone();
                    async move {
                        let deployment_id = assignment.deployment_id;
                        if let Err(error) = sync_assignment(&client, &cache_root, assignment).await
                        {
                            tracing::warn!(
                                operation = "node.serve.sync.assignment",
                                %error,
                                %deployment_id,
                                "Serve artifact sync failed"
                            );
                        }
                    }
                })
                .await;
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use axum::{Router, body::Body, response::Response, routing::get};
    use grass_node_protocol::{
        ReportServeStatusRequest, ReportServeStatusResponse, ReportedServeStatus, ServeArtifact,
        ServeAssignment, ServeAssignmentStatus, ServeResources, artifact_headers,
    };
    use tokio::sync::Mutex;
    use uuid::Uuid;

    use crate::client::ControlApiClient;

    use super::{stage_archive, sync_assignment, unique_assignments};

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "grass-serve-sync-{label}-{}",
            Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn artifact_fixture(root: &Path) -> (ServeAssignment, PathBuf) {
        let source = root.join("source");
        std::fs::create_dir_all(source.join("static")).unwrap();
        std::fs::write(
            source.join("output.toml"),
            "version = 1\n[runtime]\nkind = \"static\"\n[static]\ndirectory = \"static\"\n",
        )
        .unwrap();
        std::fs::write(source.join("static/index.html"), "<h1>ready</h1>").unwrap();
        let archive_path = root.join("artifact.zip");
        let packed = grass_archive::pack_dir(&source, &archive_path).unwrap();
        let assignment = ServeAssignment {
            deployment_id: Uuid::now_v7(),
            project_id: Uuid::now_v7(),
            runtime_kind: "static".to_owned(),
            status: ServeAssignmentStatus::Pending,
            artifact: ServeArtifact {
                artifact_id: Uuid::now_v7(),
                checksum_sha256: packed.checksum_sha256,
                packed_size_bytes: packed.size_bytes,
                unpacked_size_bytes: packed.unpacked_size_bytes,
            },
            resources: ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 256,
            },
        };
        (assignment, archive_path)
    }

    #[test]
    fn stages_verified_artifact_in_checksum_directory() {
        let root = temp_dir("success");
        let cache_root = root.join("cache");
        let (assignment, archive_path) = artifact_fixture(&root);

        let staged = stage_archive(&cache_root, &assignment, &archive_path).unwrap();

        let expected = cache_root
            .join(assignment.deployment_id.to_string())
            .join(&assignment.artifact.checksum_sha256);
        assert_eq!(staged, expected);
        assert!(expected.join("output.toml").is_file());
        assert!(expected.join(".artifact.json").is_file());
        let entries = std::fs::read_dir(expected.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!entries.iter().any(|entry| entry.starts_with(".staging-")));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stages_legacy_artifact_with_unknown_unpacked_size() {
        let root = temp_dir("legacy-size");
        let cache_root = root.join("cache");
        let (mut assignment, archive_path) = artifact_fixture(&root);
        assignment.artifact.unpacked_size_bytes = 0;

        let staged = stage_archive(&cache_root, &assignment, &archive_path).unwrap();

        assert!(staged.join("output.toml").is_file());
        assert!(staged.join(".artifact.json").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sync_batches_deduplicate_deployments() {
        let root = temp_dir("deduplicate");
        let (assignment, _) = artifact_fixture(&root);

        let unique = unique_assignments(vec![assignment.clone(), assignment]);

        assert_eq!(unique.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checksum_mismatch_preserves_existing_cache() {
        let root = temp_dir("mismatch");
        let cache_root = root.join("cache");
        let (assignment, archive_path) = artifact_fixture(&root);
        let existing = stage_archive(&cache_root, &assignment, &archive_path).unwrap();
        let mut mismatched = assignment.clone();
        mismatched.artifact.artifact_id = Uuid::now_v7();
        mismatched.artifact.checksum_sha256 = "0".repeat(64);

        let error = stage_archive(&cache_root, &mismatched, &archive_path).unwrap_err();

        assert!(error.to_string().contains("checksum mismatch"));
        assert!(existing.join(".artifact.json").is_file());
        assert!(existing.join("static/index.html").is_file());
        assert!(
            !cache_root
                .join(assignment.deployment_id.to_string())
                .join(&mismatched.artifact.checksum_sha256)
                .exists()
        );
        let entries = std::fs::read_dir(existing.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!entries.iter().any(|entry| entry.starts_with(".staging-")));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn sync_reports_status_and_cleans_download_file() {
        let root = temp_dir("sync");
        let cache_root = root.join("cache");
        let (assignment, archive_path) = artifact_fixture(&root);
        let deployment_root = cache_root.join(assignment.deployment_id.to_string());
        std::fs::create_dir_all(deployment_root.join(".staging-stale")).unwrap();
        std::fs::write(deployment_root.join(".download-stale.zip"), b"partial").unwrap();
        let archive = std::fs::read(archive_path).unwrap();
        let response_artifact = assignment.artifact.clone();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let reported = statuses.clone();
        let app = Router::new()
            .route(
                "/api/v1/internal/deployments/{deployment_id}/artifact",
                get(move || {
                    let body = archive.clone();
                    let artifact = response_artifact.clone();
                    async move {
                        Response::builder()
                            .header(
                                artifact_headers::PACKED_SIZE_BYTES,
                                artifact.packed_size_bytes,
                            )
                            .header(
                                artifact_headers::UNPACKED_SIZE_BYTES,
                                artifact.unpacked_size_bytes,
                            )
                            .header(artifact_headers::CHECKSUM_SHA256, artifact.checksum_sha256)
                            .body(Body::from(body))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/api/v1/internal/serve/deployments/{deployment_id}/status",
                axum::routing::post(
                    move |axum::Json(report): axum::Json<ReportServeStatusRequest>| {
                        let reported = reported.clone();
                        async move {
                            reported.lock().await.push(report.status);
                            axum::Json(serde_json::json!({
                                "code": 200,
                                "message": "OK",
                                "data": ReportServeStatusResponse { acknowledged: true }
                            }))
                        }
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();

        sync_assignment(&client, &cache_root, assignment.clone())
            .await
            .unwrap();

        assert_eq!(
            *statuses.lock().await,
            vec![ReportedServeStatus::Syncing, ReportedServeStatus::Ready]
        );
        assert!(
            deployment_root
                .join(assignment.artifact.checksum_sha256)
                .is_dir()
        );
        let entries = std::fs::read_dir(deployment_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!entries.iter().any(|entry| entry.starts_with(".download-")));
        assert!(!entries.iter().any(|entry| entry.starts_with(".staging-")));

        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cached_assignment_still_reports_syncing_before_ready() {
        let root = temp_dir("cached-resync");
        let cache_root = root.join("cache");
        let (assignment, archive_path) = artifact_fixture(&root);
        stage_archive(&cache_root, &assignment, &archive_path).unwrap();

        let statuses = Arc::new(Mutex::new(Vec::new()));
        let reported = statuses.clone();
        let app = Router::new().route(
            "/api/v1/internal/serve/deployments/{deployment_id}/status",
            axum::routing::post(
                move |axum::Json(report): axum::Json<ReportServeStatusRequest>| {
                    let reported = reported.clone();
                    async move {
                        reported.lock().await.push(report.status);
                        axum::Json(serde_json::json!({
                            "code": 200,
                            "message": "OK",
                            "data": ReportServeStatusResponse { acknowledged: true }
                        }))
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();

        sync_assignment(&client, &cache_root, assignment)
            .await
            .unwrap();

        assert_eq!(
            *statuses.lock().await,
            vec![ReportedServeStatus::Syncing, ReportedServeStatus::Ready]
        );

        server.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}
