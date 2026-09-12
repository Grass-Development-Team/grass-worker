//! Build log collection: every line gets a sequence number, is written to
//! the local `build-log.txt`, and is batched to the Control API. Milestone 9
//! additionally streams the same lines over the websocket channel.

use std::path::PathBuf;
use std::sync::Arc;

use grass_node_protocol::{AppendBuildLogRequest, BuildLogLine, LogStreamMessage};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::{build::realtime::RealtimePublisher, client::ControlApiClient};

const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(700);
const FLUSH_BATCH: usize = 100;
pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const QUEUE_LINES: usize = 256;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("build log exceeds its byte budget")]
    Budget,
    #[error("build log consumer is unavailable or too slow")]
    Consumer,
}

struct LogState {
    seq: u64,
    remaining: u64,
}

#[derive(Clone)]
pub struct LogCollector {
    deployment_id: Uuid,
    state: Arc<Mutex<LogState>>,
    sender: mpsc::Sender<BuildLogLine>,
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
        let (sender, mut receiver) = mpsc::channel::<BuildLogLine>(QUEUE_LINES);

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
                                    if tokio::time::timeout(IO_TIMEOUT, file.write_all(text.as_bytes())).await.is_err() { break; }
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
                let _ = tokio::time::timeout(IO_TIMEOUT, file.flush()).await;
            }
        });

        (
            Self {
                deployment_id,
                state: Arc::new(Mutex::new(LogState {
                    seq: 0,
                    remaining: MAX_LOG_BYTES,
                })),
                sender,
                realtime,
            },
            flusher,
        )
    }

    /// Records one log line under the given stage: persisted through the
    /// HTTP batch and mirrored on the realtime channel.
    pub async fn log(&self, stage: &str, line: impl AsRef<str>) -> Result<(), LogError> {
        let mut text = line.as_ref();
        let mut state = self.state.lock().await;
        loop {
            let mut length = text.len().min(MAX_LINE_BYTES);
            while !text.is_char_boundary(length) {
                length -= 1;
            }
            let chunk = &text[..length];
            // Charge framing too, so empty lines cannot evade the total budget.
            let cost = (chunk.len() + stage.len() + 64) as u64;
            let remaining = state.remaining.checked_sub(cost).ok_or(LogError::Budget)?;
            let permit = tokio::time::timeout(IO_TIMEOUT, self.sender.reserve())
                .await
                .map_err(|_| LogError::Consumer)?
                .map_err(|_| LogError::Consumer)?;
            state.seq += 1;
            state.remaining = remaining;
            let entry = BuildLogLine {
                seq: state.seq,
                stage: stage.to_owned(),
                line: chunk.to_owned(),
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
            permit.send(entry);
            text = &text[length..];
            if text.is_empty() {
                break;
            }
        }
        Ok(())
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
    if let Err(error) =
        tokio::time::timeout(IO_TIMEOUT, client.append_build_log(deployment_id, &request))
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("log flush timed out")))
    {
        tracing::warn!(
            operation = "node.build_log.flush",
            %error,
            deployment_id = %deployment_id,
            "failed to push build log batch"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collector(bytes: u64) -> (LogCollector, mpsc::Receiver<BuildLogLine>) {
        let (sender, receiver) = mpsc::channel(1);
        (
            LogCollector {
                deployment_id: Uuid::now_v7(),
                state: Arc::new(Mutex::new(LogState {
                    seq: 0,
                    remaining: bytes,
                })),
                sender,
                realtime: None,
            },
            receiver,
        )
    }
    #[tokio::test]
    async fn slow_consumers_apply_backpressure_without_losing_lines() {
        let (collector, mut receiver) = collector(1024);
        collector.log("build", "first").await.unwrap();
        let second = collector.log("build", "second");
        tokio::pin!(second);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut second)
                .await
                .is_err()
        );
        assert_eq!(receiver.len(), 1);
        assert_eq!(receiver.recv().await.unwrap().line, "first");
        second.await.unwrap();
        assert_eq!(receiver.recv().await.unwrap().line, "second");
    }
    #[tokio::test]
    async fn empty_lines_are_charged_and_closed_consumers_fail() {
        let (collector, mut receiver) = collector(128);
        collector.log("build", "").await.unwrap();
        receiver.recv().await.unwrap();
        assert!(matches!(
            collector.log("build", "").await,
            Err(LogError::Budget)
        ));
        let (collector, receiver) = self::collector(1024);
        drop(receiver);
        assert!(matches!(
            collector.log("build", "line").await,
            Err(LogError::Consumer)
        ));
    }
    #[tokio::test]
    async fn unicode_lines_are_split_without_changing_content() {
        let (collector, mut receiver) = collector(MAX_LOG_BYTES);
        let text = "好".repeat(MAX_LINE_BYTES);
        let expected = text.clone();
        let producer = tokio::spawn(async move {
            collector.log("build", text).await.unwrap();
        });
        let mut actual = String::new();
        while let Some(line) = receiver.recv().await {
            assert!(line.line.len() <= MAX_LINE_BYTES);
            actual.push_str(&line.line);
        }
        producer.await.unwrap();
        assert_eq!(actual, expected);
    }
}
