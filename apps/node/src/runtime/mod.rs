//! Container runtime abstraction.
//!
//! Builds (and future SSR services) never run raw on the host: every user
//! command executes inside a container runtime backend. The first stage
//! implements Docker and Podman socket backends (Podman speaks the Docker
//! API on its socket); Apple Container and Jail backends are reserved
//! names that fail construction until they are implemented.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::config::RuntimeConfig;

#[derive(Debug, thiserror::Error)]
pub enum ContainerRuntimeError {
    #[error("container runtime backend {0} is not implemented yet")]
    BackendNotImplemented(String),
    #[error("unknown container runtime backend {0}")]
    UnknownBackend(String),
    #[error("build was canceled")]
    Canceled,
    #[error("build timed out after {0} seconds")]
    Timeout(u64),
    #[error("container runtime request failed: {0}")]
    Runtime(String),
}

pub struct PrepareImageInput<'a> {
    pub image: &'a str,
}

pub struct RunBuildInput {
    pub image: String,
    /// Local workspace copied into the container at /workspace. The build
    /// never bind-mounts host paths, so it works identically for bare-metal
    /// and containerized Nodes (sibling-engine setups included).
    pub workspace: PathBuf,
    /// Working directory inside the container, relative to /workspace.
    pub working_dir: String,
    /// Shell script executed with `sh -lc`.
    pub script: String,
    pub env: Vec<(String, String)>,
    pub cpu_limit: u32,
    pub memory_mb: u64,
    pub network: String,
    /// Hard wall-clock limit; the container is stopped when it elapses.
    pub timeout: Option<Duration>,
    /// Paths relative to the working directory copied back into the local
    /// workspace after a successful build (missing paths are skipped).
    pub export_paths: Vec<String>,
}

#[derive(Debug)]
pub struct BuildExecutionResult {
    pub exit_code: i64,
}

/// A long-running SSR server container.
#[derive(Debug)]
pub struct RunServiceInput {
    /// Deterministic container name; an existing running container with this
    /// name is adopted instead of recreated.
    pub name: String,
    pub image: String,
    /// Local directory uploaded to `/app` inside the container.
    pub app_dir: PathBuf,
    /// Shell command executed with `sh -lc` from `/app`.
    pub start_command: String,
    pub env: Vec<(String, String)>,
    /// Port the server listens on inside the container.
    pub container_port: u16,
    pub cpu_millicores: u64,
    pub memory_mb: u64,
    pub network: String,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RunningService {
    /// `ip:port` address of the service reachable from this node process.
    pub upstream: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceContainer {
    pub name: String,
    pub labels: HashMap<String, String>,
}

pub trait ContainerRuntime: Send + Sync {
    /// Ensures the build image exists locally, pulling it when missing.
    fn prepare_image(
        &self,
        input: PrepareImageInput<'_>,
        logs: mpsc::Sender<String>,
    ) -> impl Future<Output = Result<(), ContainerRuntimeError>> + Send;

    /// Runs a build command inside an isolated container, streaming output
    /// lines into `logs` and honoring the cancel signal and timeout.
    fn run_build(
        &self,
        input: RunBuildInput,
        logs: mpsc::Sender<String>,
        cancel: watch::Receiver<bool>,
    ) -> impl Future<Output = Result<BuildExecutionResult, ContainerRuntimeError>> + Send;

    /// Starts (or adopts) the SSR service container described by `input`
    /// and returns how to reach it.
    fn run_service(
        &self,
        input: RunServiceInput,
    ) -> impl Future<Output = Result<RunningService, ContainerRuntimeError>> + Send;

    /// Lists service container names managed under a deterministic prefix.
    fn list_services(
        &self,
        prefix: &str,
    ) -> impl Future<Output = Result<Vec<ServiceContainer>, ContainerRuntimeError>> + Send;

    /// Stops and removes an SSR service container by name.
    fn stop_service(
        &self,
        service_id: &str,
    ) -> impl Future<Output = Result<(), ContainerRuntimeError>> + Send;
}

mod socket;
pub use socket::SocketRuntime;

/// Concrete runtime dispatch. Enum dispatch keeps the trait free of object
/// safety constraints while backends stay swappable.
pub enum BuildRuntime {
    Socket(SocketRuntime),
    /// Deterministic in-process fake used by tests.
    #[allow(dead_code)]
    Fake(FakeRuntime),
}

impl BuildRuntime {
    pub fn from_config(config: &RuntimeConfig) -> Result<Self, ContainerRuntimeError> {
        match config.backend.as_str() {
            "docker-socket" | "podman-socket" => Ok(Self::Socket(SocketRuntime::connect(
                &config.backend,
                &config.socket,
            )?)),
            backend @ ("apple-container" | "jail") => Err(
                ContainerRuntimeError::BackendNotImplemented(backend.to_owned()),
            ),
            other => Err(ContainerRuntimeError::UnknownBackend(other.to_owned())),
        }
    }
}

impl ContainerRuntime for BuildRuntime {
    async fn prepare_image(
        &self,
        input: PrepareImageInput<'_>,
        logs: mpsc::Sender<String>,
    ) -> Result<(), ContainerRuntimeError> {
        match self {
            Self::Socket(runtime) => runtime.prepare_image(input, logs).await,
            Self::Fake(runtime) => runtime.prepare_image(input, logs).await,
        }
    }

    async fn run_build(
        &self,
        input: RunBuildInput,
        logs: mpsc::Sender<String>,
        cancel: watch::Receiver<bool>,
    ) -> Result<BuildExecutionResult, ContainerRuntimeError> {
        match self {
            Self::Socket(runtime) => runtime.run_build(input, logs, cancel).await,
            Self::Fake(runtime) => runtime.run_build(input, logs, cancel).await,
        }
    }

    async fn run_service(
        &self,
        input: RunServiceInput,
    ) -> Result<RunningService, ContainerRuntimeError> {
        match self {
            Self::Socket(runtime) => runtime.run_service(input).await,
            Self::Fake(runtime) => runtime.run_service(input).await,
        }
    }

    async fn stop_service(&self, service_id: &str) -> Result<(), ContainerRuntimeError> {
        match self {
            Self::Socket(runtime) => runtime.stop_service(service_id).await,
            Self::Fake(runtime) => runtime.stop_service(service_id).await,
        }
    }

    async fn list_services(
        &self,
        prefix: &str,
    ) -> Result<Vec<ServiceContainer>, ContainerRuntimeError> {
        match self {
            Self::Socket(runtime) => runtime.list_services(prefix).await,
            Self::Fake(runtime) => runtime.list_services(prefix).await,
        }
    }
}

/// Test backend: scripts map to canned exit codes and output lines, letting
/// pipeline tests run without a container engine.
#[derive(Default)]
pub struct FakeRuntime {
    /// Exit code returned for scripts containing the key; unmatched scripts
    /// exit 0.
    pub failures: HashMap<String, i64>,
    /// Lines emitted for every build.
    pub output: Vec<String>,
    /// Simulated execution time, checked against timeout and cancel.
    pub delay: Option<Duration>,
    services: Mutex<HashMap<String, HashMap<String, String>>>,
}

impl ContainerRuntime for FakeRuntime {
    async fn prepare_image(
        &self,
        _input: PrepareImageInput<'_>,
        _logs: mpsc::Sender<String>,
    ) -> Result<(), ContainerRuntimeError> {
        Ok(())
    }

    async fn run_build(
        &self,
        input: RunBuildInput,
        logs: mpsc::Sender<String>,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<BuildExecutionResult, ContainerRuntimeError> {
        for line in &self.output {
            let _ = logs.send(line.clone()).await;
        }

        if let Some(delay) = self.delay {
            if let Some(timeout) = input.timeout
                && delay > timeout
            {
                tokio::time::sleep(timeout).await;
                return Err(ContainerRuntimeError::Timeout(timeout.as_secs()));
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        return Err(ContainerRuntimeError::Canceled);
                    }
                }
            }
        }
        if *cancel.borrow() {
            return Err(ContainerRuntimeError::Canceled);
        }

        let exit_code = self
            .failures
            .iter()
            .find(|(needle, _)| input.script.contains(needle.as_str()))
            .map(|(_, code)| *code)
            .unwrap_or(0);
        Ok(BuildExecutionResult { exit_code })
    }

    async fn run_service(
        &self,
        input: RunServiceInput,
    ) -> Result<RunningService, ContainerRuntimeError> {
        self.services
            .lock()
            .map_err(|_| ContainerRuntimeError::Runtime("fake service lock poisoned".to_owned()))?
            .insert(input.name, input.labels);
        Ok(RunningService {
            upstream: format!("127.0.0.1:{}", input.container_port),
        })
    }

    async fn list_services(
        &self,
        prefix: &str,
    ) -> Result<Vec<ServiceContainer>, ContainerRuntimeError> {
        let mut services = self
            .services
            .lock()
            .map_err(|_| ContainerRuntimeError::Runtime("fake service lock poisoned".to_owned()))?
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, labels)| ServiceContainer {
                name: name.clone(),
                labels: labels.clone(),
            })
            .collect::<Vec<_>>();
        services.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(services)
    }

    async fn stop_service(&self, service_id: &str) -> Result<(), ContainerRuntimeError> {
        self.services
            .lock()
            .map_err(|_| ContainerRuntimeError::Runtime("fake service lock poisoned".to_owned()))?
            .remove(service_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_backends_fail_construction_with_clear_errors() {
        let mut config = RuntimeConfig::default();
        for backend in ["apple-container", "jail"] {
            config.backend = backend.to_owned();
            let error = match BuildRuntime::from_config(&config) {
                Err(error) => error,
                Ok(_) => panic!("reserved backend must not construct"),
            };
            assert!(matches!(
                error,
                ContainerRuntimeError::BackendNotImplemented(_)
            ));
        }

        config.backend = "chroot".to_owned();
        let error = match BuildRuntime::from_config(&config) {
            Err(error) => error,
            Ok(_) => panic!("unknown backend must not construct"),
        };
        assert!(matches!(error, ContainerRuntimeError::UnknownBackend(_)));
    }

    #[tokio::test]
    async fn fake_runtime_reports_failures_and_cancellation() {
        let mut runtime = FakeRuntime::default();
        runtime.failures.insert("npm run build".to_owned(), 2);

        let (log_tx, _log_rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);
        let input = RunBuildInput {
            image: "node:22".to_owned(),
            workspace: PathBuf::from("/tmp"),
            working_dir: ".".to_owned(),
            script: "npm install && npm run build".to_owned(),
            env: Vec::new(),
            cpu_limit: 1,
            memory_mb: 512,
            network: "bridge".to_owned(),
            export_paths: Vec::new(),
            timeout: None,
        };
        let result = runtime
            .run_build(input, log_tx.clone(), cancel_rx.clone())
            .await
            .unwrap();
        assert_eq!(result.exit_code, 2);

        let (cancel_tx, cancel_rx) = watch::channel(true);
        let input = RunBuildInput {
            image: "node:22".to_owned(),
            workspace: PathBuf::from("/tmp"),
            working_dir: ".".to_owned(),
            script: "sleep".to_owned(),
            env: Vec::new(),
            cpu_limit: 1,
            memory_mb: 512,
            network: "bridge".to_owned(),
            export_paths: Vec::new(),
            timeout: None,
        };
        let error = runtime
            .run_build(input, log_tx, cancel_rx)
            .await
            .unwrap_err();
        assert!(matches!(error, ContainerRuntimeError::Canceled));
        drop(cancel_tx);
    }

    #[tokio::test]
    async fn fake_runtime_reports_services_on_loopback() {
        let runtime = FakeRuntime::default();
        let running = runtime
            .run_service(RunServiceInput {
                name: "grass-ssr-test".to_owned(),
                image: "node:22".to_owned(),
                app_dir: PathBuf::from("/tmp"),
                start_command: "node server.js".to_owned(),
                env: Vec::new(),
                container_port: 8321,
                cpu_millicores: 200,
                memory_mb: 512,
                network: "bridge".to_owned(),
                labels: HashMap::new(),
            })
            .await
            .unwrap();
        assert_eq!(running.upstream, "127.0.0.1:8321");
        assert!(runtime.stop_service("grass-ssr-test").await.is_ok());
    }
}
