//! Docker/Podman socket backend.
//!
//! Podman exposes a Docker-compatible API on its socket, so both backends
//! share this implementation; only the socket path differs.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

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

use super::{
    BuildExecutionResult, ContainerRuntimeError, PrepareImageInput, RunBuildInput, RunServiceInput,
    RunningService, ServiceContainer,
};

pub struct SocketRuntime {
    backend: String,
    docker: Docker,
}

fn runtime_error(context: &str, error: impl std::fmt::Display) -> ContainerRuntimeError {
    ContainerRuntimeError::Runtime(format!("{context}: {error}"))
}

impl SocketRuntime {
    pub fn connect(backend: &str, socket: &str) -> Result<Self, ContainerRuntimeError> {
        let path = socket.strip_prefix("unix://").unwrap_or(socket).to_owned();
        let docker = Docker::connect_with_unix(&path, 120, bollard::API_DEFAULT_VERSION)
            .map_err(|error| runtime_error("connect", error))?;
        Ok(Self {
            backend: backend.to_owned(),
            docker,
        })
    }

    #[allow(dead_code)] // Reported in diagnostics once serve logging lands.
    pub fn backend(&self) -> &str {
        &self.backend
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

/// Packs a local directory as a tar rooted at `root_name` for upload.
async fn rooted_tar(
    root_name: &'static str,
    dir: PathBuf,
) -> Result<Vec<u8>, ContainerRuntimeError> {
    tokio::task::spawn_blocking(move || {
        let mut builder = tar::Builder::new(Vec::new());
        builder.follow_symlinks(false);
        builder
            .append_dir_all(root_name, &dir)
            .map_err(|error| runtime_error("workspace archive", error))?;
        builder
            .into_inner()
            .map_err(|error| runtime_error("workspace archive", error))
    })
    .await
    .map_err(|error| runtime_error("workspace archive task", error))?
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

/// Unpacks an exported tar under `destination`; `unpack_in` rejects entries
/// that would escape it.
async fn unpack_export(destination: PathBuf, bytes: Vec<u8>) -> Result<(), ContainerRuntimeError> {
    tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&destination)
            .map_err(|error| runtime_error("export unpack", error))?;
        let mut archive = tar::Archive::new(&bytes[..]);
        archive.set_overwrite(true);
        for entry in archive
            .entries()
            .map_err(|error| runtime_error("export unpack", error))?
        {
            let mut entry = entry.map_err(|error| runtime_error("export unpack", error))?;
            entry
                .unpack_in(&destination)
                .map_err(|error| runtime_error("export unpack", error))?;
        }
        Ok(())
    })
    .await
    .map_err(|error| runtime_error("export unpack task", error))?
}

impl super::ContainerRuntime for SocketRuntime {
    async fn prepare_image(
        &self,
        input: PrepareImageInput<'_>,
        logs: mpsc::Sender<String>,
    ) -> Result<(), ContainerRuntimeError> {
        if self.docker.inspect_image(input.image).await.is_ok() {
            return Ok(());
        }

        let _ = logs
            .send(format!("pulling build image {}", input.image))
            .await;
        let mut pull = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: Some(input.image.to_owned()),
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
            .inspect_image(input.image)
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
        let tar_bytes = match rooted_tar("workspace", input.workspace.clone()).await {
            Ok(bytes) => bytes,
            Err(error) => {
                self.remove_container(&name).await;
                return Err(error);
            }
        };
        if let Err(error) = self
            .docker
            .upload_to_container(
                &name,
                Some(UploadToContainerOptions {
                    path: "/".to_owned(),
                    ..Default::default()
                }),
                bollard::body_full(tar_bytes.into()),
            )
            .await
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
        let log_task = tokio::spawn(async move {
            let mut stream = log_stream_docker.logs(
                &log_stream_name,
                Some(LogsOptions {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    ..Default::default()
                }),
            );
            let mut pending = String::new();
            while let Some(chunk) = stream.next().await {
                let Ok(output) = chunk else { break };
                let bytes = output.into_bytes();
                pending.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(index) = pending.find('\n') {
                    let line: String = pending.drain(..=index).collect();
                    let _ = logs
                        .send(line.trim_end_matches(['\n', '\r']).to_owned())
                        .await;
                }
            }
            if !pending.is_empty() {
                let _ = logs.send(pending).await;
            }
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

        let outcome = tokio::select! {
            waited = wait.next() => {
                match waited {
                    Some(Ok(body)) => Ok(BuildExecutionResult { exit_code: body.status_code }),
                    // Non-zero exits surface as an error body with the code.
                    Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. })) => {
                        Ok(BuildExecutionResult { exit_code: code })
                    }
                    Some(Err(error)) => Err(runtime_error("container wait", error)),
                    None => Err(ContainerRuntimeError::Runtime(
                        "container wait stream ended unexpectedly".to_owned(),
                    )),
                }
            }
            _ = timeout_sleep => {
                let _ = self.docker
                    .stop_container(&name, Some(StopContainerOptions { t: Some(5), ..Default::default() }))
                    .await;
                Err(ContainerRuntimeError::Timeout(
                    input.timeout.map(|t| t.as_secs()).unwrap_or_default(),
                ))
            }
            changed = cancel.changed() => {
                if changed.is_ok() && *cancel.borrow() {
                    let _ = self.docker
                        .stop_container(&name, Some(StopContainerOptions { t: Some(5), ..Default::default() }))
                        .await;
                    Err(ContainerRuntimeError::Canceled)
                } else {
                    // Sender dropped without cancel: keep waiting.
                    match wait.next().await {
                        Some(Ok(body)) => Ok(BuildExecutionResult { exit_code: body.status_code }),
                        Some(Err(bollard::errors::Error::DockerContainerWaitError { code, .. })) => {
                            Ok(BuildExecutionResult { exit_code: code })
                        }
                        Some(Err(error)) => Err(runtime_error("container wait", error)),
                        None => Err(ContainerRuntimeError::Runtime(
                            "container wait stream ended unexpectedly".to_owned(),
                        )),
                    }
                }
            }
        };

        // Give the log stream a moment to drain.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), log_task).await;

        // Copy requested outputs back out after a successful build. Missing
        // paths are normal (framework candidates), so download errors skip.
        if let Ok(result) = &outcome
            && result.exit_code == 0
        {
            for relative in &input.export_paths {
                let relative = relative.trim_matches('/');
                if relative.is_empty() || relative.split('/').any(|part| part == "..") {
                    continue;
                }
                let mut stream = self.docker.download_from_container(
                    &name,
                    Some(DownloadFromContainerOptions {
                        path: format!("{container_working_dir}/{relative}"),
                    }),
                );
                let mut bytes = Vec::new();
                let mut download_failed = false;
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(data) => bytes.extend_from_slice(&data),
                        Err(_) => {
                            download_failed = true;
                            break;
                        }
                    }
                }
                if download_failed || bytes.is_empty() {
                    continue;
                }
                let parent = Path::new(relative)
                    .parent()
                    .map(|parent| local_working_dir.join(parent))
                    .unwrap_or_else(|| local_working_dir.clone());
                let target = local_working_dir.join(relative);
                let _ = tokio::fs::remove_dir_all(&target).await;
                let _ = tokio::fs::remove_file(&target).await;
                if let Err(error) = unpack_export(parent, bytes).await {
                    self.remove_container(&name).await;
                    return Err(error);
                }
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

        let tar_bytes = match rooted_tar("app", input.app_dir.clone()).await {
            Ok(bytes) => bytes,
            Err(error) => {
                self.remove_container(&input.name).await;
                return Err(error);
            }
        };
        if let Err(error) = self
            .docker
            .upload_to_container(
                &input.name,
                Some(UploadToContainerOptions {
                    path: "/".to_owned(),
                    ..Default::default()
                }),
                bollard::body_full(tar_bytes.into()),
            )
            .await
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
