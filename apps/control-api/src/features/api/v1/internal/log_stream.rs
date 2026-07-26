//! Node → Control API websocket ingest for realtime build log frames.

use axum::{
    Extension,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use grass_node_protocol::LogStreamMessage;

use crate::{infra::http::middlewares::node_auth::AuthenticatedNode, state::ControlApiState};

/// GET /api/v1/internal/log-stream
pub async fn ingest(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    let node_name = node.name.clone();
    upgrade.on_upgrade(move |socket| pump(state, socket, node_name))
}

async fn pump(state: ControlApiState, mut socket: WebSocket, node_name: String) {
    tracing::info!(
        operation = "internal.log_stream.connected",
        node = %node_name,
        "node log stream connected"
    );

    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(frame) = serde_json::from_str::<LogStreamMessage>(&text) else {
            continue;
        };
        let deployment_id = match &frame {
            LogStreamMessage::Log { deployment_id, .. }
            | LogStreamMessage::StageChange { deployment_id, .. }
            | LogStreamMessage::Done { deployment_id, .. } => *deployment_id,
            // Subscribe/Cancel travel browser → API only.
            _ => continue,
        };
        state.log_hub.publish(deployment_id, frame);
    }

    tracing::info!(
        operation = "internal.log_stream.disconnected",
        node = %node_name,
        "node log stream disconnected"
    );
}
