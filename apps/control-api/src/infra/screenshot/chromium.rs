use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{net::TcpStream, process::Command, time::Instant};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

use super::{CaptureRequest, ScreenshotProvider};

pub struct ChromiumProvider {
    executable_path: String,
}

impl ChromiumProvider {
    pub fn new(executable_path: String) -> Self {
        Self { executable_path }
    }

    async fn capture_png(&self, request: &CaptureRequest) -> anyhow::Result<Vec<u8>> {
        let target = Url::parse(&request.url)?;
        if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
            anyhow::bail!("screenshot target must be an absolute HTTP URL");
        }
        if self.executable_path.trim().is_empty() {
            anyhow::bail!("Chromium executable path is empty");
        }

        let profile =
            std::env::temp_dir().join(format!("grass-worker-chromium-{}", Uuid::now_v7().simple()));
        tokio::fs::create_dir_all(&profile).await?;
        let mut command = Command::new(&self.executable_path);
        command
            .arg("--headless=new")
            .arg("--disable-background-networking")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--hide-scrollbars")
            .arg("--metrics-recording-only")
            .arg("--mute-audio")
            .arg("--no-default-browser-check")
            .arg("--no-first-run")
            .arg("--remote-debugging-address=127.0.0.1")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn()?;

        let capture = async {
            let port = wait_for_debugging_port(&profile, &mut child).await?;
            let target = reqwest::Client::new()
                .put(format!("http://127.0.0.1:{port}/json/new?about:blank"))
                .send()
                .await?
                .error_for_status()?
                .json::<Value>()
                .await?;
            let websocket_url = target
                .get("webSocketDebuggerUrl")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Chromium did not return a page debugger URL"))?;
            let (socket, _) = connect_async(websocket_url).await?;
            let mut cdp = Cdp::new(socket, Url::parse(&request.url)?);
            cdp.command("Page.enable", json!({})).await?;
            cdp.command("Network.enable", json!({})).await?;
            cdp.command("Page.setLifecycleEventsEnabled", json!({ "enabled": true }))
                .await?;
            cdp.command(
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": 1280,
                    "height": 720,
                    "deviceScaleFactor": 1,
                    "mobile": false,
                }),
            )
            .await?;
            cdp.command(
                "Fetch.enable",
                json!({
                    "patterns": [{ "urlPattern": "*", "requestStage": "Request" }],
                }),
            )
            .await?;
            let cookie = cdp
                .command(
                    "Network.setCookie",
                    json!({
                        "name": request.cookie_name,
                        "value": request.cookie_value,
                        "url": request.url,
                        "path": "/",
                        "secure": request.url.starts_with("https://"),
                        "httpOnly": true,
                        "sameSite": "Lax",
                    }),
                )
                .await?;
            if cookie.get("success").and_then(Value::as_bool) == Some(false) {
                anyhow::bail!("Chromium rejected the preview access cookie");
            }
            cdp.command("Page.navigate", json!({ "url": request.url }))
                .await?;
            cdp.wait_until_settled().await?;
            let screenshot = cdp
                .command(
                    "Page.captureScreenshot",
                    json!({
                        "format": "png",
                        "fromSurface": true,
                        "captureBeyondViewport": false,
                    }),
                )
                .await?;
            let encoded = screenshot
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("Chromium returned no screenshot data"))?;
            STANDARD.decode(encoded).map_err(Into::into)
        };
        let result = tokio::time::timeout(Duration::from_secs(30), capture)
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Chromium screenshot capture timed out")));

        let _ = child.kill().await;
        let _ = child.wait().await;
        let _ = tokio::fs::remove_dir_all(profile).await;
        result
    }
}

fn request_is_allowed(target: &Url, candidate: &str) -> bool {
    if candidate.starts_with("data:") {
        return true;
    }
    if let Some(inner) = candidate.strip_prefix("blob:") {
        return Url::parse(inner).is_ok_and(|url| url.origin() == target.origin());
    }
    let Ok(candidate) = Url::parse(candidate) else {
        return false;
    };
    candidate.origin() == target.origin()
}

#[async_trait]
impl ScreenshotProvider for ChromiumProvider {
    async fn capture(&self, request: CaptureRequest) -> anyhow::Result<Vec<u8>> {
        self.capture_png(&request).await
    }
}

async fn wait_for_debugging_port(
    profile: &Path,
    child: &mut tokio::process::Child,
) -> anyhow::Result<u16> {
    let active_port = profile.join("DevToolsActivePort");
    for _ in 0..100 {
        if let Ok(content) = tokio::fs::read_to_string(&active_port).await
            && let Some(port) = content.lines().next()
        {
            return port.parse().map_err(Into::into);
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("Chromium exited before CDP was ready: {status}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("Chromium CDP endpoint did not become ready")
}

type ChromiumSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Cdp {
    socket: ChromiumSocket,
    next_id: u64,
    target: Url,
}

impl Cdp {
    fn new(socket: ChromiumSocket, target: Url) -> Self {
        Self {
            socket,
            next_id: 1,
            target,
        }
    }

    async fn send(&mut self, method: &str, params: Value) -> anyhow::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket
            .send(Message::Text(
                json!({ "id": id, "method": method, "params": params })
                    .to_string()
                    .into(),
            ))
            .await?;
        Ok(id)
    }

    async fn command(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let expected_id = self.send(method, params).await?;
        loop {
            let message = self.next_message().await?;
            if self.handle_paused_request(&message).await? {
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(expected_id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                anyhow::bail!("Chromium CDP {method} failed: {error}");
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn wait_until_settled(&mut self) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let message = tokio::time::timeout_at(deadline, self.next_message())
                .await
                .map_err(|_| anyhow::anyhow!("deployment preview did not finish loading"))??;
            if self.handle_paused_request(&message).await? {
                continue;
            }
            if message.get("method").and_then(Value::as_str) == Some("Page.lifecycleEvent")
                && matches!(
                    message.pointer("/params/name").and_then(Value::as_str),
                    Some("networkIdle")
                )
            {
                tokio::time::sleep(Duration::from_millis(500)).await;
                return Ok(());
            }
        }
    }

    async fn next_message(&mut self) -> anyhow::Result<Value> {
        loop {
            let message = self
                .socket
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("Chromium closed the CDP connection"))??;
            match message {
                Message::Text(text) => return serde_json::from_str(&text).map_err(Into::into),
                Message::Binary(bytes) => {
                    return serde_json::from_slice(&bytes).map_err(Into::into);
                }
                Message::Close(_) => anyhow::bail!("Chromium closed the CDP connection"),
                Message::Ping(payload) => self.socket.send(Message::Pong(payload)).await?,
                Message::Pong(_) | Message::Frame(_) => {}
            }
        }
    }

    async fn handle_paused_request(&mut self, message: &Value) -> anyhow::Result<bool> {
        if message.get("method").and_then(Value::as_str) != Some("Fetch.requestPaused") {
            return Ok(false);
        }
        let request_id = message
            .pointer("/params/requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Chromium paused a request without an id"))?;
        let url = message
            .pointer("/params/request/url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if request_is_allowed(&self.target, url) {
            self.send("Fetch.continueRequest", json!({ "requestId": request_id }))
                .await?;
        } else {
            self.send(
                "Fetch.failRequest",
                json!({ "requestId": request_id, "errorReason": "BlockedByClient" }),
            )
            .await?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use axum::{Router, response::Html, routing::get};
    use image::{ImageFormat, ImageReader};

    use super::*;

    #[test]
    fn request_interception_allows_only_the_target_origin_and_inline_resources() {
        let target = Url::parse("https://preview.example.test/").unwrap();

        assert!(request_is_allowed(
            &target,
            "https://preview.example.test/app.js"
        ));
        assert!(request_is_allowed(&target, "data:image/png;base64,AA=="));
        assert!(request_is_allowed(
            &target,
            "blob:https://preview.example.test/id"
        ));
        assert!(!request_is_allowed(
            &target,
            "https://cdn.example.test/app.js"
        ));
        assert!(!request_is_allowed(
            &target,
            "blob:https://cdn.example.test/id"
        ));
        assert!(!request_is_allowed(
            &target,
            "http://preview.example.test/app.js"
        ));
        assert!(!request_is_allowed(&target, "file:///etc/passwd"));
    }

    #[tokio::test]
    #[ignore = "requires GRASS_TEST_CHROMIUM"]
    async fn chromium_captures_a_1280_by_720_png() {
        let executable = std::env::var("GRASS_TEST_CHROMIUM")
            .expect("GRASS_TEST_CHROMIUM must point to a Chromium executable");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/",
            get(|| async {
                Html(
                    "<!doctype html><style>html,body{margin:0;width:100%;height:100%;background:rgb(12,34,56)}</style>",
                )
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let png = ChromiumProvider::new(executable)
            .capture_png(&CaptureRequest {
                url: format!("http://{address}/"),
                cookie_name: "gw_preview_access".to_owned(),
                cookie_value: "test-grant".to_owned(),
            })
            .await
            .unwrap();
        server.abort();

        let image = ImageReader::with_format(Cursor::new(png), ImageFormat::Png)
            .decode()
            .unwrap()
            .into_rgb8();
        assert_eq!(image.dimensions(), (1280, 720));
        assert_eq!(image.get_pixel(640, 360).0, [12, 34, 56]);
    }
}
