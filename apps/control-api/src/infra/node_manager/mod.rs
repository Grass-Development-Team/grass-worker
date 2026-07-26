//! Local Node process manager.
//!
//! When the platform runs API and Node on the same machine, the Control API
//! can supervise the local `grass-node` process itself: the node config is
//! generated once at Node creation time (the only moment the plaintext token
//! exists), the binary is spawned with its output piped into our logs, and
//! unexpected exits are restarted with backoff. Administrators control the
//! process through the admin Nodes API.

pub mod config_file;

use std::{path::Path, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use serde::Serialize;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};
use tracing::{info, warn};

use crate::infra::config::node_manager::NodeManagerConfig;

/// Exits faster than this count as crash-looping.
const RAPID_EXIT_WINDOW_SECONDS: i64 = 10;
/// Consecutive rapid exits before the manager gives up.
const RAPID_EXIT_LIMIT: u32 = 5;
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const DEFAULT_BASE_BACKOFF: Duration = Duration::from_secs(1);
/// How often the monitor task polls the child for exit.
const MONITOR_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Stopped,
    Running,
    Backoff,
    Failed,
}

/// Serializable snapshot of the managed process for admin responses.
#[derive(Clone, Debug, Serialize)]
pub struct ProcessStatus {
    pub state: ProcessState,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub last_exit_at: Option<String>,
    pub message: Option<String>,
}

struct ManagerInner {
    binary: String,
    args: Vec<String>,
    config_path: String,
    restart_on_exit: bool,
    base_backoff: Duration,
    child: Option<Child>,
    desired_running: bool,
    /// Incremented on every start/stop so stale monitor tasks abandon.
    generation: u64,
    state: ProcessState,
    pid: Option<u32>,
    started_at: Option<OffsetDateTime>,
    restart_count: u32,
    rapid_exits: u32,
    backoff: Duration,
    last_exit_code: Option<i32>,
    last_exit_at: Option<OffsetDateTime>,
    message: Option<String>,
}

#[derive(Clone)]
pub struct NodeManager {
    inner: Arc<Mutex<ManagerInner>>,
}

impl NodeManager {
    pub fn new(config: &NodeManagerConfig) -> Self {
        Self::with_command(
            config.local_node_binary.clone(),
            vec!["--config".to_owned(), config.local_node_config.clone()],
            config.local_node_config.clone(),
            config.restart_on_exit,
            DEFAULT_BASE_BACKOFF,
        )
    }

    fn with_command(
        binary: String,
        args: Vec<String>,
        config_path: String,
        restart_on_exit: bool,
        base_backoff: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ManagerInner {
                binary,
                args,
                config_path,
                restart_on_exit,
                base_backoff,
                child: None,
                desired_running: false,
                generation: 0,
                state: ProcessState::Stopped,
                pid: None,
                started_at: None,
                restart_count: 0,
                rapid_exits: 0,
                backoff: base_backoff,
                last_exit_code: None,
                last_exit_at: None,
                message: None,
            })),
        }
    }

    pub async fn status(&self) -> ProcessStatus {
        status_snapshot(&*self.inner.lock().await)
    }

    pub async fn config_path(&self) -> String {
        self.inner.lock().await.config_path.clone()
    }

    /// Starts the local node process. Idempotent while running.
    pub async fn start(&self) -> anyhow::Result<ProcessStatus> {
        let mut inner = self.inner.lock().await;
        if inner.state == ProcessState::Running && inner.child.is_some() {
            return Ok(status_snapshot(&inner));
        }
        if !Path::new(&inner.config_path).exists() {
            bail!(
                "local node config {} not found; create a node with the local process option to generate it",
                inner.config_path
            );
        }
        inner.desired_running = true;
        inner.generation += 1;
        inner.rapid_exits = 0;
        inner.restart_count = 0;
        inner.backoff = inner.base_backoff;
        inner.message = None;
        self.spawn_child(&mut inner)?;
        Ok(status_snapshot(&inner))
    }

    /// Stops the local node process and disables restarts until started again.
    pub async fn stop(&self) -> ProcessStatus {
        let mut inner = self.inner.lock().await;
        inner.desired_running = false;
        inner.generation += 1;
        if let Some(mut child) = inner.child.take() {
            let _ = child.start_kill();
            // Reap in the background so we never leave a zombie.
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
        }
        inner.state = ProcessState::Stopped;
        inner.pid = None;
        inner.message = Some("stopped by administrator".to_owned());
        info!(
            operation = "node_manager.stopped",
            "local node process stopped"
        );
        status_snapshot(&inner)
    }

    pub async fn restart(&self) -> anyhow::Result<ProcessStatus> {
        self.stop().await;
        self.start().await
    }

    /// Kills the child on Control API shutdown.
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;
        inner.desired_running = false;
        inner.generation += 1;
        if let Some(mut child) = inner.child.take() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        inner.state = ProcessState::Stopped;
        inner.pid = None;
    }

    fn spawn_child(&self, inner: &mut ManagerInner) -> anyhow::Result<()> {
        let mut command = Command::new(&inner.binary);
        command
            .args(&inner.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn local node binary {}", inner.binary))?;

        if let Some(stdout) = child.stdout.take() {
            tokio::spawn(pipe_child_output(stdout, false));
        }
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(pipe_child_output(stderr, true));
        }

        inner.pid = child.id();
        inner.child = Some(child);
        inner.state = ProcessState::Running;
        inner.started_at = Some(OffsetDateTime::now_utc());
        info!(
            operation = "node_manager.started",
            pid = inner.pid,
            "local node process started"
        );

        let manager = self.clone();
        let generation = inner.generation;
        tokio::spawn(async move { manager.monitor(generation).await });
        Ok(())
    }

    /// Watches one spawned child until it exits or the generation changes.
    async fn monitor(self, generation: u64) {
        loop {
            tokio::time::sleep(MONITOR_POLL).await;
            let mut inner = self.inner.lock().await;
            if inner.generation != generation {
                return;
            }
            let Some(child) = inner.child.as_mut() else {
                return;
            };
            let exit = match child.try_wait() {
                Ok(None) => continue,
                Ok(Some(status)) => status,
                Err(error) => {
                    warn!(
                        operation = "node_manager.monitor_failed",
                        %error,
                        "failed to poll local node process"
                    );
                    return;
                }
            };

            inner.child = None;
            inner.pid = None;
            let now = OffsetDateTime::now_utc();
            let ran_for = inner.started_at.map(|started| now - started);
            inner.last_exit_code = exit.code();
            inner.last_exit_at = Some(now);
            let rapid = ran_for
                .is_some_and(|lived| lived < time::Duration::seconds(RAPID_EXIT_WINDOW_SECONDS));
            inner.rapid_exits = if rapid { inner.rapid_exits + 1 } else { 0 };

            if !(inner.desired_running && inner.restart_on_exit) {
                inner.state = ProcessState::Stopped;
                inner.message = Some(format!("process exited with code {:?}", exit.code()));
                warn!(
                    operation = "node_manager.exited",
                    code = exit.code(),
                    "local node process exited"
                );
                return;
            }

            if inner.rapid_exits >= RAPID_EXIT_LIMIT {
                inner.state = ProcessState::Failed;
                inner.desired_running = false;
                inner.message = Some(
                    "local node process keeps exiting immediately; check its configuration"
                        .to_owned(),
                );
                warn!(
                    operation = "node_manager.crash_loop",
                    code = exit.code(),
                    "local node process is crash-looping; giving up"
                );
                return;
            }

            let delay = if rapid {
                inner.backoff
            } else {
                inner.base_backoff
            };
            inner.backoff = (delay * 2).min(MAX_BACKOFF);
            inner.state = ProcessState::Backoff;
            inner.restart_count += 1;
            inner.message = Some(format!("restarting after exit with code {:?}", exit.code()));
            warn!(
                operation = "node_manager.restarting",
                code = exit.code(),
                delay_ms = delay.as_millis() as u64,
                "local node process exited; restarting"
            );
            drop(inner);

            tokio::time::sleep(delay).await;
            let mut inner = self.inner.lock().await;
            if inner.generation != generation || !inner.desired_running {
                return;
            }
            if let Err(error) = self.spawn_child(&mut inner) {
                inner.state = ProcessState::Failed;
                inner.desired_running = false;
                inner.message = Some(format!("failed to respawn local node: {error}"));
                warn!(
                    operation = "node_manager.respawn_failed",
                    %error,
                    "failed to respawn local node process"
                );
            }
            // A new monitor task was spawned for the respawned child (same
            // generation); this task ends either way.
            return;
        }
    }
}

async fn pipe_child_output(reader: impl tokio::io::AsyncRead + Unpin, is_stderr: bool) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if is_stderr {
            warn!(operation = "node_manager.node_output", "{line}");
        } else {
            info!(operation = "node_manager.node_output", "{line}");
        }
    }
}

fn status_snapshot(inner: &ManagerInner) -> ProcessStatus {
    ProcessStatus {
        state: inner.state,
        pid: inner.pid,
        started_at: inner.started_at.map(format_timestamp),
        restart_count: inner.restart_count,
        last_exit_code: inner.last_exit_code,
        last_exit_at: inner.last_exit_at.map(format_timestamp),
        message: inner.message.clone(),
    }
}

fn format_timestamp(at: OffsetDateTime) -> String {
    at.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| at.unix_timestamp().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager_for(command: &str, restart_on_exit: bool, config_path: &str) -> NodeManager {
        NodeManager::with_command(
            "/bin/sh".to_owned(),
            vec!["-c".to_owned(), command.to_owned()],
            config_path.to_owned(),
            restart_on_exit,
            Duration::from_millis(10),
        )
    }

    fn temp_config() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "grass-node-manager-test-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "").unwrap();
        path
    }

    async fn wait_for(manager: &NodeManager, predicate: impl Fn(&ProcessStatus) -> bool) -> bool {
        for _ in 0..250 {
            if predicate(&manager.status().await) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn start_and_stop_manage_a_long_running_process() {
        let config = temp_config();
        let manager = manager_for("sleep 30", true, config.to_str().unwrap());

        let status = manager.start().await.unwrap();
        assert_eq!(status.state, ProcessState::Running);
        assert!(status.pid.is_some());

        let status = manager.stop().await;
        assert_eq!(status.state, ProcessState::Stopped);
        assert!(status.pid.is_none());
        std::fs::remove_file(config).unwrap();
    }

    #[tokio::test]
    async fn missing_config_file_refuses_to_start() {
        let manager = manager_for("sleep 1", true, "/nonexistent/grass-node-test.toml");
        assert!(manager.start().await.is_err());
    }

    #[tokio::test]
    async fn crash_looping_process_restarts_then_fails() {
        let config = temp_config();
        let manager = manager_for("exit 7", true, config.to_str().unwrap());
        manager.start().await.unwrap();

        assert!(
            wait_for(&manager, |status| status.state == ProcessState::Failed).await,
            "crash loop should end in failed state"
        );
        let status = manager.status().await;
        assert!(status.restart_count >= 1);
        assert_eq!(status.last_exit_code, Some(7));
        std::fs::remove_file(config).unwrap();
    }

    #[tokio::test]
    async fn clean_exit_without_restart_reports_stopped() {
        let config = temp_config();
        let manager = manager_for("exit 0", false, config.to_str().unwrap());
        manager.start().await.unwrap();

        assert!(
            wait_for(&manager, |status| status.state == ProcessState::Stopped).await,
            "process exit should be observed"
        );
        let status = manager.status().await;
        assert_eq!(status.restart_count, 0);
        assert_eq!(status.last_exit_code, Some(0));
        std::fs::remove_file(config).unwrap();
    }
}
