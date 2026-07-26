//! Realtime build log relay.
//!
//! Nodes push log frames over one websocket; browsers subscribe per
//! deployment. The hub fans frames out through per-deployment broadcast
//! channels. Persistence stays on the HTTP build-log path — the hub is a
//! transport, not a store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use grass_node_protocol::LogStreamMessage;
use tokio::sync::broadcast;
use uuid::Uuid;

const CHANNEL_CAPACITY: usize = 1024;

#[derive(Clone, Default)]
pub struct LogStreamHub {
    channels: Arc<Mutex<HashMap<Uuid, broadcast::Sender<LogStreamMessage>>>>,
}

impl LogStreamHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Publishes a frame to every subscriber of the deployment. Terminal
    /// `Done` frames drop the channel so finished deployments do not leak
    /// senders.
    pub fn publish(&self, deployment_id: Uuid, message: LogStreamMessage) {
        let is_done = matches!(message, LogStreamMessage::Done { .. });
        let sender = {
            let mut channels = self.channels.lock().unwrap();
            if is_done {
                channels.remove(&deployment_id)
            } else {
                Some(
                    channels
                        .entry(deployment_id)
                        .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
                        .clone(),
                )
            }
        };
        if let Some(sender) = sender {
            // Zero receivers is fine; frames simply go nowhere.
            let _ = sender.send(message);
        }
    }

    pub fn subscribe(&self, deployment_id: Uuid) -> broadcast::Receiver<LogStreamMessage> {
        let mut channels = self.channels.lock().unwrap();
        channels
            .entry(deployment_id)
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
            .subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_reach_every_subscriber() {
        let hub = LogStreamHub::new();
        let deployment_id = Uuid::now_v7();
        let mut first = hub.subscribe(deployment_id);
        let mut second = hub.subscribe(deployment_id);

        hub.publish(
            deployment_id,
            LogStreamMessage::StageChange {
                deployment_id,
                stage: "build".to_owned(),
            },
        );

        for receiver in [&mut first, &mut second] {
            let frame = receiver.recv().await.unwrap();
            assert!(matches!(frame, LogStreamMessage::StageChange { .. }));
        }
    }

    #[tokio::test]
    async fn done_frames_close_the_channel() {
        let hub = LogStreamHub::new();
        let deployment_id = Uuid::now_v7();
        let mut receiver = hub.subscribe(deployment_id);

        hub.publish(
            deployment_id,
            LogStreamMessage::Done {
                deployment_id,
                build_status: "ready".to_owned(),
            },
        );

        let frame = receiver.recv().await.unwrap();
        assert!(matches!(frame, LogStreamMessage::Done { .. }));
        // Channel dropped: the next recv errors with Closed.
        assert!(receiver.recv().await.is_err());
        assert!(hub.channels.lock().unwrap().is_empty());
    }
}
