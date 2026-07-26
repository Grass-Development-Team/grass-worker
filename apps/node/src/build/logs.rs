//! Build log collection: every line gets a sequence number, is written to
//! the local `build-log.txt`, and is batched to the Control API. Milestone 9
//! additionally streams the same lines over the websocket channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use grass_node_protocol::{AppendBuildLogRequest, BuildLogLine, LogStreamMessage};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{build::realtime::RealtimePublisher, client::ControlApiClient};

const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(700);
const FLUSH_BATCH: usize = 100;

#[derive(Clone)]
pub struct LogCollector {
    deployment_id: Uuid,
    seq: Arc<AtomicU64>,
    sender: mpsc::UnboundedSender<BuildLogLine>,
    realtime: Option<RealtimePublisher>,
}

impl LogCollector {
    /// Creates a collector plus its background flusher. Dropping every clone
    /// of the collector lets the flusher drain and finish; await the handle
    /// to be sure all lines reached the Control API.
    pub fn start(
        deployment_id: Uuid,
        client: ControlApiClient,
        local_log_path: PathBuf,
        realtime: Option<RealtimePublisher>,
    ) -> (Self, tokio::task::JoinHandle<()>) {
        let (sender, mut receiver) = mpsc::unbounded_channel::<BuildLogLine>();

        let flusher = tokio::spawn(async move {
            if let Some(parent) = local_log_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&local_log_path)
                .await
                .ok();

            let mut batch: Vec<BuildLogLine> = Vec::new();
            let mut interval = tokio::time::interval(FLUSH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    line = receiver.recv() => {
                        match line {
                            Some(line) => {
                                if let Some(file) = file.as_mut() {
                                    let text = format!("[{}] {}\n", line.stage, line.line);
                                    let _ = file.write_all(text.as_bytes()).await;
                                }
                                batch.push(line);
                                if batch.len() >= FLUSH_BATCH {
                                    flush(&client, deployment_id, &mut batch).await;
                                }
                            }
                            None => break,
                        }
                    }
                    _ = interval.tick() => {
                        flush(&client, deployment_id, &mut batch).await;
                    }
                }
            }

            flush(&client, deployment_id, &mut batch).await;
            if let Some(file) = file.as_mut() {
                let _ = file.flush().await;
            }
        });

        (
            Self {
                deployment_id,
                seq: Arc::new(AtomicU64::new(0)),
                sender,
                realtime,
            },
            flusher,
        )
    }

    #[allow(dead_code)] // Read by the websocket log pusher in Milestone 9.
    pub fn deployment_id(&self) -> Uuid {
        self.deployment_id
    }

    /// Records one log line under the given stage: persisted through the
    /// HTTP batch and mirrored on the realtime channel.
    pub fn log(&self, stage: &str, line: impl Into<String>) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let entry = BuildLogLine {
            seq,
            stage: stage.to_owned(),
            line: line.into(),
            timestamp_ms: now_ms(),
        };
        if let Some(realtime) = &self.realtime {
            realtime.publish(LogStreamMessage::Log {
                deployment_id: self.deployment_id,
                stage: entry.stage.clone(),
                line: entry.line.clone(),
                timestamp_ms: entry.timestamp_ms,
                seq: entry.seq,
            });
        }
        let _ = self.sender.send(entry);
    }

    /// Announces a stage change on the realtime channel.
    pub fn publish_stage(&self, stage: &str) {
        if let Some(realtime) = &self.realtime {
            realtime.publish(LogStreamMessage::StageChange {
                deployment_id: self.deployment_id,
                stage: stage.to_owned(),
            });
        }
    }

    /// Announces the terminal build status on the realtime channel.
    pub fn publish_done(&self, build_status: &str) {
        if let Some(realtime) = &self.realtime {
            realtime.publish(LogStreamMessage::Done {
                deployment_id: self.deployment_id,
                build_status: build_status.to_owned(),
            });
        }
    }

    #[allow(dead_code)] // Read by the websocket log pusher in Milestone 9.
    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::Relaxed)
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

async fn flush(client: &ControlApiClient, deployment_id: Uuid, batch: &mut Vec<BuildLogLine>) {
    if batch.is_empty() {
        return;
    }
    let lines = std::mem::take(batch);
    let request = AppendBuildLogRequest { lines };
    if let Err(error) = client.append_build_log(deployment_id, &request).await {
        tracing::warn!(
            operation = "node.build_log.flush",
            %error,
            deployment_id = %deployment_id,
            "failed to push build log batch"
        );
    }
}
