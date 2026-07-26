//! Node → Control API realtime log frames over websocket.
//!
//! Persistence uses the HTTP build-log batches; this channel only carries
//! the live view. Frames produced while the socket is down are dropped —
//! browsers recover through the HTTP catch-up endpoint.

use futures_util::SinkExt;
use grass_node_protocol::LogStreamMessage;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue};
use tracing::{debug, warn};

#[derive(Clone)]
pub struct RealtimePublisher {
    sender: mpsc::UnboundedSender<LogStreamMessage>,
}

impl RealtimePublisher {
    /// Spawns the websocket forwarder and returns the publish handle. The
    /// task ends when every publisher clone is dropped.
    pub fn start(control_api: &str, token: &str) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let url = websocket_url(control_api);
        let token = token.to_owned();
        tokio::spawn(forward(url, token, receiver));
        Self { sender }
    }

    pub fn publish(&self, message: LogStreamMessage) {
        let _ = self.sender.send(message);
    }
}

fn websocket_url(control_api: &str) -> String {
    let base = control_api.trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{base}")
    };
    format!("{ws_base}/api/v1/internal/log-stream")
}

async fn forward(
    url: String,
    token: String,
    mut receiver: mpsc::UnboundedReceiver<LogStreamMessage>,
) {
    let mut socket = None;

    while let Some(message) = receiver.recv().await {
        let Ok(text) = serde_json::to_string(&message) else {
            continue;
        };

        if socket.is_none() {
            socket = connect(&url, &token).await;
        }
        if let Some(active) = socket.as_mut() {
            if active
                .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
                .await
                .is_err()
            {
                // One immediate reconnect attempt per frame; otherwise the
                // frame is dropped and catch-up covers the gap.
                socket = connect(&url, &token).await;
                if let Some(active) = socket.as_mut() {
                    let Ok(text) = serde_json::to_string(&message) else {
                        continue;
                    };
                    let _ = active
                        .send(tokio_tungstenite::tungstenite::Message::Text(text.into()))
                        .await;
                }
            }
        }
    }

    if let Some(mut active) = socket {
        let _ = active.close(None).await;
    }
    debug!(
        operation = "node.realtime.closed",
        "realtime log channel closed"
    );
}

async fn connect(
    url: &str,
    token: &str,
) -> Option<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let mut request = match url.into_client_request() {
        Ok(request) => request,
        Err(error) => {
            warn!(operation = "node.realtime.request", %error, "invalid websocket url");
            return None;
        }
    };
    let Ok(header) = HeaderValue::from_str(&format!("Bearer {token}")) else {
        return None;
    };
    request.headers_mut().insert(
        tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
        header,
    );

    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Some(socket),
        Err(error) => {
            warn!(operation = "node.realtime.connect", %error, "websocket connect failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_urls_map_schemes() {
        assert_eq!(
            websocket_url("http://127.0.0.1:7817"),
            "ws://127.0.0.1:7817/api/v1/internal/log-stream"
        );
        assert_eq!(
            websocket_url("https://api.grass.test/"),
            "wss://api.grass.test/api/v1/internal/log-stream"
        );
    }
}
