//! Docker/Podman socket backend.
//!
//! Podman exposes a Docker-compatible API on its socket, so both backends
//! share this implementation; only the socket path differs.

use std::{collections::HashMap, path::Path};

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, ContainerInspectResponse, HostConfig, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, DownloadFromContainerOptions,
    InspectContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions, UploadToContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};

use super::archive::{self, Budget, LIMITS, TransferControl};
use tokio_util::io::ReaderStream;

use super::{
    BuildExecutionResult, ContainerRuntimeError, RunBuildInput, RunServiceInput, RunningService,
    ServiceContainer,
};

pub struct SocketRuntime {
    docker: Docker,
}

fn runtime_error(context: &str, error: impl std::fmt::Display) -> ContainerRuntimeError {
    ContainerRuntimeError::Runtime(format!("{context}: {error}"))
}

impl SocketRuntime {
    pub fn connect(socket: &str) -> Result<Self, ContainerRuntimeError> {
        let path = socket.strip_prefix("unix://").unwrap_or(socket).to_owned();
        let docker = Docker::connect_with_unix(&path, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| runtime_error("connect", error))?;
        Ok(Self { docker })
    }

    async fn remove_container(&self, name: &str) {
        let _ = self
            .docker
            .remove_container(
                name,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await;
    }
}

#[cfg(test)]
async fn rooted_tar(
    root_name: &'static str,
    dir: std::path::PathBuf,
) -> Result<tokio::fs::File, ContainerRuntimeError> {
    let (_, cancel) = watch::channel(false);
    archive::pack(root_name, dir, LIMITS, TransferControl::new(cancel)).await
}

/// Address of a service container as seen from this node process: the
/// container IP on the configured network (containers on the same bridge or
/// user network reach each other directly, no published ports needed).
fn upstream_address(
    inspect: &ContainerInspectResponse,
    network: &str,
    port: u16,
) -> Option<String> {
    let networks = inspect.network_settings.as_ref()?.networks.as_ref()?;
    let endpoint = networks.get(network).or_else(|| networks.values().next())?;
    let ip = endpoint.ip_address.as_deref().filter(|ip| !ip.is_empty())?;
    Some(format!("{ip}:{port}"))
}

fn service_nano_cpus(cpu_millicores: u64) -> i64 {
    i64::try_from(cpu_millicores)
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000_000)
}

fn service_memory_bytes(memory_mb: u64) -> i64 {
    memory_mb.saturating_mul(1024 * 1024).min(i64::MAX as u64) as i64
}

/// Split before decoding or allocating a String. Raw chunks and final collector lines are bounded.
async fn forward_logs<S, B, E>(
    mut stream: S,
    logs: mpsc::Sender<String>,
    max_bytes: usize,
) -> Result<(), ContainerRuntimeError>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let mut remaining = max_bytes;
    let mut pending = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| runtime_error("build log stream", error))?;
        remaining = remaining
            .checked_sub(chunk.as_ref().len())
            .ok_or_else(|| runtime_error("build logs", "byte budget exceeded"))?;
        for &byte in chunk.as_ref() {
            if byte == b'\n' {
                if pending.last() == Some(&b'\r') {
                    pending.pop();
                }
                logs.send(String::from_utf8_lossy(&pending).into_owned())
                    .await
                    .map_err(|_| runtime_error("build logs", "consumer closed"))?;
                pending.clear();
            } else {
                pending.push(byte);
                if pending.len() >= crate::build::logs::MAX_LINE_BYTES / 3 {
                    let length = match std::str::from_utf8(&pending) {
                        Err(error) if error.error_len().is_none() => error.valid_up_to(),
                        _ => pending.len(),
                    };
                    logs.send(String::from_utf8_lossy(&pending[..length]).into_owned())
                        .await
                        .map_err(|_| runtime_error("build logs", "consumer closed"))?;
                    pending.drain(..length);
                }
            }
        }
    }
    if !pending.is_empty() {
        logs.send(String::from_utf8_lossy(&pending).into_owned())
            .await
            .map_err(|_| runtime_error("build logs", "consumer closed"))?;
    }
    Ok(())
}

impl super::ContainerRuntime for SocketRuntime {
    async fn prepare_image(
        &self,
        image: &str,
        logs: mpsc::Sender<String>,
    ) -> Result<(), ContainerRuntimeError> {
        if self.docker.inspect_image(image).await.is_ok() {
            return Ok(());
        }

        let _ = logs.send(format!("pulling build image {}", image)).await;
        let mut pull = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(image.to_owned()),
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(progress) = pull.next().await {
            let progress = progress.map_err(|error| runtime_error("image pull", error))?;
            if let Some(status) = progress.status
                && status.contains("Downloaded")
            {
                let _ = logs.send(status).await;
            }
        }
        // Confirm the image exists after the pull stream completes.
        self.docker
            .inspect_image(image)
            .await
            .map(|_| ())
            .map_err(|error| runtime_error("image inspect", error))
    }

    async fn run_build(
        &self,
        input: RunBuildInput,
        logs: mpsc::Sender<String>,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<BuildExecutionResult, ContainerRuntimeError> {
        let name = format!("grass-build-{}", uuid::Uuid::now_v7().simple());
        let working_dir = if input.working_dir.trim().is_empty() || input.working_dir == "." {
            "/workspace".to_owned()
        } else {
            format!("/workspace/{}", input.working_dir.trim_matches('/'))
        };
        let local_working_dir = if input.working_dir.trim().is_empty() || input.working_dir == "." {
            input.workspace.clone()
        } else {
            input.workspace.join(input.working_dir.trim_matches('/'))
        };
        let container_working_dir = working_dir.clone();

        let env: Vec<String> = input
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();

        let config = ContainerCreateBody {
            image: Some(input.image.clone()),
            cmd: Some(vec![
                "/bin/sh".to_owned(),
                "-lc".to_owned(),
                input.script.clone(),
            ]),
            working_dir: Some(working_dir),
            env: Some(env),
            host_config: Some(HostConfig {
                memory: Some((input.memory_mb * 1024 * 1024) as i64),
                nano_cpus: Some(i64::from(input.cpu_limit) * 1_000_000_000),
                network_mode: Some(input.network.clone()),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    ..Default::default()
                }),
                config,
            )
            .await
            .map_err(|error| runtime_error("container create", error))?;

        // Copy the workspace in instead of bind-mounting it: the engine may
        // live on another host (containerized Node), where host paths from
        // this process do not exist.
        let tar_bytes = match archive::pack(
            "workspace",
            input.workspace.clone(),
            LIMITS,
            TransferControl::new(cancel.clone()),
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(error) => {
                self.remove_container(&name).await;
                return Err(error);
            }
        };
        if let Err(error) = TransferControl::new(cancel.clone())
            .run(self.docker.upload_to_container(
                &name,
                Some(UploadToContainerOptions {
                    path: "/".to_owned(),
                    ..Default::default()
                }),
                bollard::body_try_stream(ReaderStream::with_capacity(tar_bytes, 64 * 1024)),
            ))
            .await
            .and_then(|result| result.map_err(|error| runtime_error("workspace upload", error)))
        {
            self.remove_container(&name).await;
            return Err(runtime_error("workspace upload", error));
        }

        if let Err(error) = self
            .docker
            .start_container(&name, None::<StartContainerOptions>)
            .await
        {
            self.remove_container(&name).await;
            return Err(runtime_error("container start", error));
        }

        // Stream stdout/stderr lines while the build runs.
        let log_stream_docker = self.docker.clone();
        let log_stream_name = name.clone();
        let mut log_task = tokio::spawn(async move {
            let stream = log_stream_docker
                .logs(
                    &log_stream_name,
                    Some(LogsOptions {
                        follow: true,
                        stdout: true,
                        stderr: true,
                        ..Default::default()
                    }),
                )
                .map(|result| result.map(|output| output.into_bytes()));
            forward_logs(stream, logs, 16 * 1024 * 1024).await
        });
        let mut wait = self
            .docker
            .wait_container(&name, None::<WaitContainerOptions>);
        let timeout_sleep = async {
            match input.timeout {
                Some(timeout) => tokio::time::sleep(timeout).await,
                None => std::future::pending::<()>().await,
            }
        };
        tokio::pin!(timeout_sleep);
        let mut logs_finished = false;
        let mut cancel_open = true;
        let mut outcome = loop {
            if *cancel.borrow() {
                break Err(ContainerRuntimeError::Canceled);
            }
            tokio::select! {
                waited = wait.next() => break match waited {
                    Some(Ok(body)) => Ok(BuildExecutionResult { exit_code: body.status_code }),
                    Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. })) => Ok(BuildExecutionResult { exit_code: code }),
                    Some(Err(error)) => Err(runtime_error("container wait", error)),
                    None => Err(runtime_error("container wait", "stream ended unexpectedly")),
                },
                logged = &mut log_task, if !logs_finished => {
                    logs_finished = true;
                    match logged {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => break Err(error),
                        Err(error) => break Err(runtime_error("build log task", error)),
                    }
                }
                _ = &mut timeout_sleep => break Err(ContainerRuntimeError::Timeout(input.timeout.map(|t| t.as_secs()).unwrap_or_default())),
                changed = cancel.changed(), if cancel_open => { if changed.is_err() { cancel_open = false; } }
            }
        };
        if outcome.is_err() {
            self.remove_container(&name).await;
        }
        if !logs_finished {
            match tokio::time::timeout(std::time::Duration::from_secs(20), &mut log_task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) if outcome.is_ok() => outcome = Err(error),
                Ok(Err(error)) if outcome.is_ok() => {
                    outcome = Err(runtime_error("build log task", error))
                }
                Err(_) => {
                    log_task.abort();
                    let _ = log_task.await;
                    if outcome.is_ok() {
                        outcome = Err(runtime_error("build logs", "drain timed out"));
                    }
                }
                _ => {}
            }
        }

        // A shared budget spans all framework candidates, including SSR dependencies.
        if outcome.as_ref().is_ok_and(|result| result.exit_code == 0) {
            let exported = async {
                let control = TransferControl::new(cancel.clone());
                let mut budget = Budget::new(LIMITS);
                for relative in &input.export_paths {
                    let relative = relative.trim_matches('/');
                    if relative.is_empty()
                        || relative.split('/').any(|part| part == ".." || part == ".")
                    {
                        return Err(runtime_error("export path", "unsafe output path"));
                    }
                    let stream = self.docker.download_from_container(
                        &name,
                        Some(DownloadFromContainerOptions {
                            path: format!("{container_working_dir}/{relative}"),
                        }),
                    );
                    let Some(file) = archive::download(stream, &mut budget, &control).await? else {
                        continue;
                    };
                    let root = Path::new(relative)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(|| runtime_error("export path", "invalid output name"))?
                        .to_owned();
                    let staging = tempfile::tempdir_in(&input.workspace)
                        .map_err(|error| runtime_error("export staging", error))?;
                    budget = archive::unpack(
                        file,
                        staging.path().into(),
                        root.clone(),
                        budget,
                        control.clone(),
                    )
                    .await?;
                    let parent = Path::new(relative)
                        .parent()
                        .unwrap_or_else(|| Path::new(""));
                    let mut target_parent = local_working_dir.clone();
                    for component in parent.components() {
                        target_parent.push(component);
                        if std::fs::symlink_metadata(&target_parent)
                            .is_ok_and(|metadata| metadata.is_symlink())
                        {
                            return Err(runtime_error(
                                "export path",
                                "output parent is a symbolic link",
                            ));
                        }
                    }
                    tokio::fs::create_dir_all(&target_parent)
                        .await
                        .map_err(|error| runtime_error("export parent", error))?;
                    let target = local_working_dir.join(relative);
                    let _ = tokio::fs::remove_dir_all(&target).await;
                    let _ = tokio::fs::remove_file(&target).await;
                    tokio::fs::rename(staging.path().join(root), target)
                        .await
                        .map_err(|error| runtime_error("export install", error))?;
                }
                Ok::<(), ContainerRuntimeError>(())
            }
            .await;
            if let Err(error) = exported {
                outcome = Err(error);
            }
        }

        self.remove_container(&name).await;

        outcome
    }

    async fn run_service(
        &self,
        input: RunServiceInput,
    ) -> Result<RunningService, ContainerRuntimeError> {
        // Adopt a running container left over from a previous node process;
        // recreate anything stopped or unreachable.
        if let Ok(existing) = self
            .docker
            .inspect_container(&input.name, None::<InspectContainerOptions>)
            .await
        {
            let running = existing
                .state
                .as_ref()
                .and_then(|state| state.running)
                .unwrap_or(false);
            let labels_match = input.labels.iter().all(|(name, value)| {
                existing
                    .config
                    .as_ref()
                    .and_then(|config| config.labels.as_ref())
                    .and_then(|labels| labels.get(name))
                    == Some(value)
            });
            if running
                && labels_match
                && let Some(upstream) =
                    upstream_address(&existing, &input.network, input.container_port)
            {
                return Ok(RunningService { upstream });
            }
            self.remove_container(&input.name).await;
        }

        let env: Vec<String> = input
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        let config = ContainerCreateBody {
            image: Some(input.image.clone()),
            cmd: Some(vec![
                "/bin/sh".to_owned(),
                "-lc".to_owned(),
                input.start_command.clone(),
            ]),
            working_dir: Some("/app".to_owned()),
            env: Some(env),
            host_config: Some(HostConfig {
                memory: Some(service_memory_bytes(input.memory_mb)),
                nano_cpus: Some(service_nano_cpus(input.cpu_millicores)),
                network_mode: Some(input.network.clone()),
                restart_policy: Some(RestartPolicy {
                    name: Some(RestartPolicyNameEnum::ON_FAILURE),
                    maximum_retry_count: Some(3),
                }),
                ..Default::default()
            }),
            labels: Some(input.labels.clone()),
            ..Default::default()
        };
        self.docker
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(input.name.clone()),
                    ..Default::default()
                }),
                config,
            )
            .await
            .map_err(|error| runtime_error("service create", error))?;

        let tar_bytes = match archive::pack(
            "app",
            input.app_dir.clone(),
            LIMITS,
            TransferControl::new(watch::channel(false).1),
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(error) => {
                self.remove_container(&input.name).await;
                return Err(error);
            }
        };
        if let Err(error) = TransferControl::new(watch::channel(false).1)
            .run(self.docker.upload_to_container(
                &input.name,
                Some(UploadToContainerOptions {
                    path: "/".to_owned(),
                    ..Default::default()
                }),
                bollard::body_try_stream(ReaderStream::with_capacity(tar_bytes, 64 * 1024)),
            ))
            .await
            .and_then(|result| result.map_err(|error| runtime_error("service upload", error)))
        {
            self.remove_container(&input.name).await;
            return Err(runtime_error("service upload", error));
        }

        if let Err(error) = self
            .docker
            .start_container(&input.name, None::<StartContainerOptions>)
            .await
        {
            self.remove_container(&input.name).await;
            return Err(runtime_error("service start", error));
        }

        let inspected = self
            .docker
            .inspect_container(&input.name, None::<InspectContainerOptions>)
            .await
            .map_err(|error| runtime_error("service inspect", error))?;
        let Some(upstream) = upstream_address(&inspected, &input.network, input.container_port)
        else {
            self.remove_container(&input.name).await;
            return Err(ContainerRuntimeError::Runtime(format!(
                "service container has no IP address on network {}",
                input.network
            )));
        };
        Ok(RunningService { upstream })
    }

    async fn stop_service(&self, service_id: &str) -> Result<(), ContainerRuntimeError> {
        let _ = self
            .docker
            .stop_container(
                service_id,
                Some(StopContainerOptions {
                    t: Some(5),
                    ..Default::default()
                }),
            )
            .await;
        self.remove_container(service_id).await;
        Ok(())
    }

    async fn list_services(
        &self,
        prefix: &str,
    ) -> Result<Vec<ServiceContainer>, ContainerRuntimeError> {
        let mut filters = HashMap::new();
        filters.insert("name".to_owned(), vec![prefix.to_owned()]);
        let containers = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            }))
            .await
            .map_err(|error| runtime_error("service list", error))?;
        let mut services = containers
            .into_iter()
            .flat_map(|container| {
                let labels = container.labels.unwrap_or_default();
                container
                    .names
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |name| ServiceContainer {
                        name: name.trim_start_matches('/').to_owned(),
                        labels: labels.clone(),
                    })
            })
            .filter(|service| service.name.starts_with(prefix))
            .collect::<Vec<_>>();
        services.sort_by(|left, right| left.name.cmp(&right.name));
        services.dedup_by(|left, right| left.name == right.name);
        Ok(services)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_DOCKER_SOCKET and a local alpine:3.22 image"]
    async fn docker_build_exports_logs_and_failure_cleanup() -> anyhow::Result<()> {
        use crate::runtime::ContainerRuntime;
        use std::time::Duration;

        let runtime = SocketRuntime::connect(&std::env::var("GRASS_TEST_DOCKER_SOCKET")?)?;
        // A unique marker identifies only this test's containers if an assertion fails.
        let marker = format!("GW_SECURITY_TEST_MARKER={}", uuid::Uuid::now_v7());
        let result: anyhow::Result<()> = async {
            for (scenario, script) in [
                ("success", "mkdir -p dist .output/server; cp input.txt dist/index.html; printf 'server' > .output/server/index.mjs; echo built"),
                ("log-limit", "head -c 17825792 /dev/zero | tr '\\000' x; sleep 30"),
                ("timeout", "sleep 30"),
                ("symlink", "ln -s /tmp dist"),
            ] {
                let workspace = tempfile::tempdir()?;
                std::fs::write(workspace.path().join("input.txt"), b"site contents")?;
                let (sender, mut receiver) = mpsc::channel::<String>(2);
                let consumer = tokio::spawn(async move {
                    let mut bytes = 0;
                    while let Some(line) = receiver.recv().await {
                        assert!(line.len() <= crate::build::logs::MAX_LINE_BYTES);
                        bytes += line.len();
                    }
                    bytes
                });
                let (cancel_sender, cancel) = watch::channel(false);
                // Closing the cancel channel must not disable the wall-clock deadline.
                drop(cancel_sender);
                let outcome = runtime.run_build(RunBuildInput {
                    image: "alpine:3.22".into(),
                    workspace: workspace.path().into(),
                    working_dir: ".".into(),
                    script: script.into(),
                    env: vec![("GW_SECURITY_TEST_MARKER".into(), marker.split_once('=').unwrap().1.into())],
                    cpu_limit: 1,
                    memory_mb: 64,
                    network: "none".into(),
                    timeout: Some(if scenario == "timeout" { Duration::from_millis(100) } else { Duration::from_secs(15) }),
                    export_paths: vec!["dist".into(), ".output".into(), "missing-output".into()],
                }, sender, cancel).await;
                let bytes = consumer.await?;
                match scenario {
                    "success" => {
                        anyhow::ensure!(outcome?.exit_code == 0);
                        anyhow::ensure!(std::fs::read(workspace.path().join("dist/index.html"))? == b"site contents");
                        anyhow::ensure!(std::fs::read(workspace.path().join(".output/server/index.mjs"))? == b"server");
                        anyhow::ensure!(bytes > 0);
                    }
                    "log-limit" => anyhow::ensure!(outcome.unwrap_err().to_string().contains("byte budget exceeded")),
                    "timeout" => anyhow::ensure!(matches!(outcome, Err(ContainerRuntimeError::Timeout(_)))),
                    _ => anyhow::ensure!(outcome.is_err(), "symbolic link output root was accepted"),
                }
                anyhow::ensure!(!std::fs::read_dir(workspace.path())?.filter_map(Result::ok)
                    .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp")), "export staging directory leaked");
            }
            Ok(())
        }.await;

        let containers = runtime
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                ..Default::default()
            }))
            .await?;
        let mut leaked = 0;
        for container in containers {
            let Some(id) = container.id else { continue };
            let inspect = runtime
                .docker
                .inspect_container(&id, None::<InspectContainerOptions>)
                .await?;
            if inspect
                .config
                .and_then(|config| config.env)
                .is_some_and(|env| env.contains(&marker))
            {
                leaked += 1;
                runtime.remove_container(&id).await;
            }
        }
        result?;
        anyhow::ensure!(leaked == 0, "runtime left {leaked} test containers behind");
        Ok(())
    }

    #[tokio::test]
    async fn upload_archives_preserve_sparse_files_as_regular_entries() {
        use std::io::{Read, Seek, SeekFrom, Write};

        let source = tempfile::tempdir().unwrap();
        let mut file = std::fs::File::create(source.path().join("sparse.bin")).unwrap();
        let mut expected = vec![0; 1024 * 1024 + 4];
        expected[..4].copy_from_slice(b"head");
        expected[1024 * 1024..].copy_from_slice(b"tail");
        file.write_all(b"head").unwrap();
        file.seek(SeekFrom::Start(1024 * 1024)).unwrap();
        file.write_all(b"tail").unwrap();
        file.sync_all().unwrap();
        std::fs::write(source.path().join("ordinary.txt"), b"ordinary contents").unwrap();

        for root in ["workspace", "app"] {
            let bytes = rooted_tar(root, source.path().to_owned()).await.unwrap();
            let mut archive = tar::Archive::new(bytes.into_std().await);
            let mut files = HashMap::new();
            for entry in archive.entries().unwrap() {
                let mut entry = entry.unwrap();
                if entry.header().entry_type().is_dir() {
                    continue;
                }
                assert_eq!(entry.header().entry_type(), tar::EntryType::Regular);
                let path = entry.path().unwrap().into_owned();
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents).unwrap();
                files.insert(path, contents);
            }
            assert_eq!(files.len(), 2);
            assert_eq!(files[&Path::new(root).join("sparse.bin")], expected);
            assert_eq!(
                files[&Path::new(root).join("ordinary.txt")],
                b"ordinary contents"
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upload_archives_preserve_symlinks_without_reading_their_targets() {
        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), b"outside workspace").unwrap();
        std::os::unix::fs::symlink(outside.path(), source.path().join("link")).unwrap();

        let bytes = rooted_tar("workspace", source.path().to_owned())
            .await
            .unwrap();
        let mut archive = tar::Archive::new(bytes.into_std().await);
        let mut links = 0;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            if entry.header().entry_type().is_dir() {
                continue;
            }
            assert_eq!(entry.header().entry_type(), tar::EntryType::Symlink);
            assert_eq!(entry.path().unwrap(), Path::new("workspace/link"));
            assert_eq!(entry.link_name().unwrap().unwrap(), outside.path());
            assert_eq!(entry.size(), 0);
            links += 1;
        }
        assert_eq!(links, 1);
    }

    #[test]
    fn service_cpu_millicores_convert_to_docker_nano_cpus() {
        assert_eq!(service_nano_cpus(200), 200_000_000);
        assert_eq!(service_nano_cpus(50), 50_000_000);
    }

    #[test]
    fn service_memory_megabytes_convert_without_overflow() {
        assert_eq!(service_memory_bytes(256), 268_435_456);
        assert_eq!(service_memory_bytes(u64::MAX), i64::MAX);
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[tokio::test]
    async fn long_unterminated_logs_are_split_with_bounded_lines() {
        let text = "构建".repeat(16 * 1024);
        let stream = futures_util::stream::iter(
            text.as_bytes()
                .chunks(511)
                .map(|chunk| Ok::<_, std::io::Error>(chunk.to_vec()))
                .collect::<Vec<_>>(),
        );
        let (sender, mut receiver) = mpsc::channel(2);
        let task = tokio::spawn(forward_logs(stream, sender, text.len()));
        let mut actual = String::new();
        while let Some(line) = receiver.recv().await {
            assert!(line.len() <= crate::build::logs::MAX_LINE_BYTES);
            actual.push_str(&line);
        }
        task.await.unwrap().unwrap();
        assert_eq!(actual, text);
    }
    #[tokio::test]
    async fn log_budget_and_consumer_failure_stop_the_producer() {
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(b"12345".to_vec())]);
        let (sender, _receiver) = mpsc::channel(1);
        assert!(forward_logs(stream, sender, 4).await.is_err());
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(b"line\n".to_vec())]);
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        assert!(forward_logs(stream, sender, 1024).await.is_err());
    }
}
