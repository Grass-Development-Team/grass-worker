//! Docker/Podman socket backend.
//!
//! Podman exposes a Docker-compatible API on its socket, so both backends
//! share this implementation; only the socket path differs.

use bollard::Docker;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions, WaitContainerOptions,
};
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};

use super::{
    BuildExecutionResult, ContainerRuntimeError, PrepareImageInput, RunBuildInput, RunServiceInput,
    RunningService,
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
        let workspace = input.workspace.display().to_string();
        let working_dir = if input.working_dir.trim().is_empty() || input.working_dir == "." {
            "/workspace".to_owned()
        } else {
            format!("/workspace/{}", input.working_dir.trim_matches('/'))
        };

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
                binds: Some(vec![format!("{workspace}:/workspace")]),
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

        // Give the log stream a moment to drain, then clean up.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), log_task).await;
        self.remove_container(&name).await;

        outcome
    }

    async fn run_service(
        &self,
        _input: RunServiceInput,
    ) -> Result<RunningService, ContainerRuntimeError> {
        Err(ContainerRuntimeError::BackendNotImplemented(
            "run_service (SSR)".to_owned(),
        ))
    }

    async fn stop_service(&self, _service_id: &str) -> Result<(), ContainerRuntimeError> {
        Err(ContainerRuntimeError::BackendNotImplemented(
            "stop_service (SSR)".to_owned(),
        ))
    }
}
