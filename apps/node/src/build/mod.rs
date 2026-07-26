//! Build loop and pipeline: claim → checkout → install/build in the
//! container runtime → Grass Output generation → archive → upload → report.

pub mod git;
pub mod logs;
pub mod realtime;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use grass_node_protocol::{ClaimRequest, ClaimedDeployment, ReportedStatus, StageRequest, stage};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::{
    client::ControlApiClient,
    config::NodeConfig,
    output,
    runtime::{
        BuildRuntime, ContainerRuntime, ContainerRuntimeError, PrepareImageInput, RunBuildInput,
    },
};

const CLAIM_INTERVAL: Duration = Duration::from_secs(5);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct BuildLoop {
    pub client: ControlApiClient,
    pub config: NodeConfig,
    pub runtime: Arc<BuildRuntime>,
    pub active_builds: Arc<AtomicU16>,
}

impl BuildLoop {
    /// Polls for claimable deployments and spawns a build task per claim,
    /// bounded by the configured concurrency.
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(CLAIM_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;

                let active = self.active_builds.load(Ordering::Relaxed);
                let capacity = self.config.build.concurrency.saturating_sub(active);
                if capacity == 0 {
                    continue;
                }

                let claim = match self.client.claim(&ClaimRequest { capacity }).await {
                    Ok(response) => response.deployment,
                    Err(error) => {
                        warn!(operation = "node.claim.failed", %error, "claim request failed");
                        continue;
                    }
                };
                let Some(claimed) = claim else { continue };

                info!(
                    operation = "node.claimed",
                    deployment_id = %claimed.deployment_id,
                    "claimed deployment"
                );

                self.active_builds.fetch_add(1, Ordering::Relaxed);
                let client = self.client.clone();
                let config = self.config.clone();
                let runtime = self.runtime.clone();
                let active_builds = self.active_builds.clone();
                tokio::spawn(async move {
                    run_build_job(client, config, runtime, claimed).await;
                    active_builds.fetch_sub(1, Ordering::Relaxed);
                });
            }
        })
    }
}

struct BuildFailure {
    code: &'static str,
    message: String,
}

impl BuildFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

async fn run_build_job(
    client: ControlApiClient,
    config: NodeConfig,
    runtime: Arc<BuildRuntime>,
    claimed: ClaimedDeployment,
) {
    let deployment_id = claimed.deployment_id;
    let workspace = PathBuf::from(&config.node.work_root)
        .join("builds")
        .join(deployment_id.to_string());
    let publisher =
        realtime::RealtimePublisher::start(&config.node.control_api, &config.node.node_token);
    let (collector, log_flusher) = logs::LogCollector::start(
        deployment_id,
        client.clone(),
        workspace.join("build-log.txt"),
        Some(publisher),
    );

    // Cancel watcher: polls the stage endpoint so user cancels reach the
    // container quickly even between progress reports.
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let cancel_client = client.clone();
    let cancel_poll = tokio::spawn(async move {
        let mut interval = tokio::time::interval(CANCEL_POLL_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match cancel_client
                .report_stage(
                    deployment_id,
                    &StageRequest {
                        status: None,
                        stage: None,
                        failure_code: None,
                        failure_message: None,
                        build_minutes: None,
                    },
                )
                .await
            {
                Ok(response) if response.cancel_requested => {
                    let _ = cancel_tx.send(true);
                    break;
                }
                _ => {}
            }
        }
    });

    let started = Instant::now();
    let outcome = run_pipeline(
        &client,
        &config,
        runtime.as_ref(),
        &claimed,
        &workspace,
        &collector,
        cancel_rx.clone(),
    )
    .await;
    let build_minutes = (started.elapsed().as_secs().div_ceil(60)).max(1) as i64;

    cancel_poll.abort();

    let (status, failure) = match outcome {
        Ok(()) => (ReportedStatus::Ready, None),
        Err(failure) if failure.code == "canceled" => (ReportedStatus::Canceled, Some(failure)),
        Err(failure) => (ReportedStatus::Failed, Some(failure)),
    };

    if let Some(failure) = &failure {
        collector.log("system", format!("build failed: {}", failure.message));
    } else {
        collector.log("system", "build completed successfully");
    }

    let status_value = match status {
        ReportedStatus::Ready => "ready",
        ReportedStatus::Canceled => "canceled",
        _ => "failed",
    };
    collector.publish_done(status_value);

    let report = StageRequest {
        status: Some(status),
        stage: None,
        failure_code: failure.as_ref().map(|failure| failure.code.to_owned()),
        failure_message: failure.as_ref().map(|failure| failure.message.clone()),
        build_minutes: Some(build_minutes),
    };
    if let Err(error) = client.report_stage(deployment_id, &report).await {
        warn!(
            operation = "node.build.report_failed",
            %error,
            deployment_id = %deployment_id,
            "failed to report terminal build status"
        );
    }

    // Drain remaining log lines to the Control API before cleanup.
    drop(collector);
    let _ = log_flusher.await;

    let keep_workspace =
        config.build.retain_workspace_on_failure && !matches!(status, ReportedStatus::Ready);
    if !keep_workspace {
        let _ = tokio::fs::remove_dir_all(&workspace).await;
    }

    info!(
        operation = "node.build.finished",
        deployment_id = %deployment_id,
        status = ?status,
        build_minutes,
        "build job finished"
    );
}

fn stage_report(status: Option<ReportedStatus>, stage_name: &str) -> StageRequest {
    StageRequest {
        status,
        stage: Some(stage_name.to_owned()),
        failure_code: None,
        failure_message: None,
        build_minutes: None,
    }
}

async fn report_stage_checked(
    client: &ControlApiClient,
    deployment_id: uuid::Uuid,
    request: &StageRequest,
) -> Result<(), BuildFailure> {
    match client.report_stage(deployment_id, request).await {
        Ok(response) if response.cancel_requested => {
            Err(BuildFailure::new("canceled", "build canceled by user"))
        }
        Ok(_) => Ok(()),
        Err(error) => {
            // Losing contact with the Control API is not fatal for the build
            // itself; keep going and let the next report retry.
            warn!(operation = "node.stage.report_failed", %error, "stage report failed");
            Ok(())
        }
    }
}

/// Detects the package manager from lockfiles for default commands.
fn default_commands(project_root: &Path) -> (String, String) {
    if project_root.join("bun.lock").is_file() || project_root.join("bun.lockb").is_file() {
        ("bun install".to_owned(), "bun run build".to_owned())
    } else if project_root.join("pnpm-lock.yaml").is_file() {
        (
            "corepack enable >/dev/null 2>&1 || true; pnpm install --frozen-lockfile".to_owned(),
            "pnpm run build".to_owned(),
        )
    } else if project_root.join("yarn.lock").is_file() {
        (
            "corepack enable >/dev/null 2>&1 || true; yarn install --frozen-lockfile".to_owned(),
            "yarn build".to_owned(),
        )
    } else {
        ("npm install".to_owned(), "npm run build".to_owned())
    }
}

async fn run_pipeline(
    client: &ControlApiClient,
    config: &NodeConfig,
    runtime: &BuildRuntime,
    claimed: &ClaimedDeployment,
    workspace: &Path,
    collector: &logs::LogCollector,
    cancel: watch::Receiver<bool>,
) -> Result<(), BuildFailure> {
    let deployment_id = claimed.deployment_id;

    // The Control API refuses non-static runtimes at creation; this guard
    // keeps the Node safe against protocol drift.
    if claimed.runtime_kind != "static" {
        return Err(BuildFailure::new(
            "runtime_not_implemented",
            format!("{} runtime is not implemented yet", claimed.runtime_kind),
        ));
    }

    report_stage_checked(
        client,
        deployment_id,
        &stage_report(Some(ReportedStatus::Queued), stage::QUEUED),
    )
    .await?;

    // --- Checkout -----------------------------------------------------------
    report_stage_checked(
        client,
        deployment_id,
        &stage_report(Some(ReportedStatus::Building), stage::CHECKOUT),
    )
    .await?;
    collector.publish_stage(stage::CHECKOUT);
    collector.log(
        stage::CHECKOUT,
        format!("cloning {}", claimed.repository_url),
    );

    let checkout = git::checkout(
        &claimed.repository_url,
        claimed.branch.as_deref(),
        claimed.commit_hash.as_deref(),
        claimed.root_directory.as_deref(),
        workspace,
    )
    .await
    .map_err(|error| match &error {
        git::CheckoutError::InvalidRootDirectory(_) => {
            BuildFailure::new("invalid_root_directory", error.to_string())
        }
        _ => BuildFailure::new("git_clone_failed", error.to_string()),
    })?;
    if let Some(commit) = &checkout.commit_hash {
        collector.log(stage::CHECKOUT, format!("checked out {commit}"));
    }

    // Custom Grass Output is a later-stage capability: refuse early when the
    // repository ships its own manifest.
    if checkout
        .project_root
        .join(".grass/output/output.toml")
        .is_file()
    {
        return Err(BuildFailure::new(
            "custom_output_unsupported",
            "Custom Grass Output is not supported in the first stage",
        ));
    }

    // --- Install and build inside the container runtime ---------------------
    let (default_install, default_build) = default_commands(&checkout.project_root);
    let install_command = claimed
        .install_command
        .clone()
        .filter(|command| !command.trim().is_empty())
        .unwrap_or(default_install);
    let build_command = claimed
        .build_command
        .clone()
        .filter(|command| !command.trim().is_empty())
        .unwrap_or(default_build);

    let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
    let image = config.runtime.default_build_image.clone();

    let image_collector = collector.clone();
    let image_pump = tokio::spawn(async move {
        while let Some(line) = log_rx.recv().await {
            image_collector.log(stage::BUILD, line);
        }
    });
    runtime
        .prepare_image(PrepareImageInput { image: &image }, log_tx.clone())
        .await
        .map_err(|error| BuildFailure::new("image_pull_failed", error.to_string()))?;
    drop(log_tx);
    let _ = image_pump.await;

    let timeout = claimed
        .build_timeout_seconds
        .filter(|seconds| *seconds > 0)
        .map(|seconds| Duration::from_secs(seconds as u64));

    // Install and build run in ONE container so state (node_modules) carries
    // over without host-path sharing; sentinel exit codes keep failure
    // attribution while real exit codes stay visible in the log.
    let script = format!(
        "( {install_command} ); rc=$?; \
if [ $rc -ne 0 ]; then echo \"install command failed with exit code $rc\"; exit 91; fi; \
( {build_command} ); rc=$?; \
if [ $rc -ne 0 ]; then echo \"build command failed with exit code $rc\"; exit 92; fi"
    );

    report_stage_checked(client, deployment_id, &stage_report(None, stage::BUILD)).await?;
    collector.publish_stage(stage::BUILD);
    collector.log(stage::BUILD, format!("$ {install_command}"));
    collector.log(stage::BUILD, format!("$ {build_command}"));

    let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
    let pump_collector = collector.clone();
    let pump = tokio::spawn(async move {
        while let Some(line) = log_rx.recv().await {
            pump_collector.log(stage::BUILD, line);
        }
    });

    // Every output location the detector may look at, copied back after a
    // successful build.
    let mut export_paths: Vec<String> = Vec::new();
    if let Some(configured) = claimed
        .output_directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != ".")
    {
        export_paths.push(configured.trim_matches('/').to_owned());
    }
    for candidate in ["dist", "build", "out", ".output", "public", "_site"] {
        if !export_paths.iter().any(|existing| existing == candidate) {
            export_paths.push(candidate.to_owned());
        }
    }

    let result = runtime
        .run_build(
            RunBuildInput {
                image: image.clone(),
                workspace: checkout.source_dir.clone(),
                working_dir: git::container_working_dir(
                    &checkout.source_dir,
                    &checkout.project_root,
                ),
                script,
                env: vec![("CI".to_owned(), "true".to_owned())],
                cpu_limit: config.runtime.resources.cpu_limit,
                memory_mb: config.runtime.resources.memory_mb,
                network: config.runtime.network.clone(),
                timeout,
                export_paths,
            },
            log_tx,
            cancel.clone(),
        )
        .await;
    let _ = pump.await;

    match result {
        Ok(result) if result.exit_code == 0 => {}
        Ok(result) if result.exit_code == 91 => {
            return Err(BuildFailure::new(
                "install_failed",
                "install command failed",
            ));
        }
        Ok(result) => {
            let _ = result;
            return Err(BuildFailure::new("build_failed", "build command failed"));
        }
        Err(ContainerRuntimeError::Canceled) => {
            return Err(BuildFailure::new("canceled", "build canceled by user"));
        }
        Err(ContainerRuntimeError::Timeout(seconds)) => {
            return Err(BuildFailure::new(
                "build_timeout",
                format!("build exceeded the {seconds}s limit"),
            ));
        }
        Err(error) => {
            return Err(BuildFailure::new("runtime_failed", error.to_string()));
        }
    }

    // --- Grass Output -------------------------------------------------------
    report_stage_checked(client, deployment_id, &stage_report(None, stage::OUTPUT)).await?;
    collector.publish_stage(stage::OUTPUT);
    collector.log(stage::OUTPUT, "generating .grass/output");

    let project_root = checkout.project_root.clone();
    let configured_output = claimed.output_directory.clone();
    let build_command_for_manifest = build_command.clone();
    let generated = tokio::task::spawn_blocking(move || {
        output::generate_grass_output(
            &project_root,
            configured_output.as_deref(),
            Some(&build_command_for_manifest),
        )
    })
    .await
    .map_err(|error| BuildFailure::new("output_failed", error.to_string()))?
    .map_err(|error| match &error {
        output::OutputError::CustomOutputUnsupported => {
            BuildFailure::new("custom_output_unsupported", error.to_string())
        }
        output::OutputError::RuntimeNotImplemented(_) => {
            BuildFailure::new("runtime_not_implemented", error.to_string())
        }
        _ => BuildFailure::new("output_invalid", error.to_string()),
    })?;
    collector.log(
        stage::OUTPUT,
        format!(
            "grass output ready (framework: {}, spa_fallback: {})",
            generated.framework_name, generated.spa_fallback
        ),
    );

    // --- Archive ------------------------------------------------------------
    report_stage_checked(client, deployment_id, &stage_report(None, stage::ARCHIVE)).await?;
    collector.publish_stage(stage::ARCHIVE);
    let archive_path = workspace.join("grass-output.zip");
    let output_root = generated.output_root.clone();
    let archive_target = archive_path.clone();
    let packed =
        tokio::task::spawn_blocking(move || grass_archive::pack_dir(&output_root, &archive_target))
            .await
            .map_err(|error| BuildFailure::new("archive_failed", error.to_string()))?
            .map_err(|error| BuildFailure::new("archive_failed", error.to_string()))?;
    collector.log(
        stage::ARCHIVE,
        format!(
            "packed {} files ({} bytes, sha256 {})",
            packed.file_count, packed.size_bytes, packed.checksum_sha256
        ),
    );

    // --- Upload -------------------------------------------------------------
    report_stage_checked(client, deployment_id, &stage_report(None, stage::UPLOAD)).await?;
    collector.publish_stage(stage::UPLOAD);
    let bytes = tokio::fs::read(&archive_path)
        .await
        .map_err(|error| BuildFailure::new("upload_failed", error.to_string()))?;
    let uploaded = client
        .upload_artifact(
            deployment_id,
            bytes,
            "static",
            "1",
            Some(&generated.framework_name),
            (!generated.framework_version.is_empty())
                .then_some(generated.framework_version.as_str()),
        )
        .await
        .map_err(|error| BuildFailure::new("upload_failed", error.to_string()))?;
    collector.log(
        stage::UPLOAD,
        format!(
            "artifact uploaded ({} bytes, sha256 {})",
            uploaded.size_bytes, uploaded.checksum_sha256
        ),
    );

    // Keep a local unpacked copy for the serve path on this node.
    let artifact_cache =
        PathBuf::from(&config.serve.artifact_cache_root).join(deployment_id.to_string());
    let cache_archive = archive_path.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if artifact_cache.exists() {
            let _ = std::fs::remove_dir_all(&artifact_cache);
        }
        grass_archive::unpack_zip(&cache_archive, &artifact_cache)
    })
    .await;

    Ok(())
}
