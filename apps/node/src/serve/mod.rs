//! Public static site serving.
//!
//! Every public request resolves its Host header through the Control API
//! (with a short-lived local cache), locates the deployment's unpacked
//! Grass Output, and serves files from the manifest's static directory with
//! index handling, SPA fallback, and strict path normalization.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{client::ControlApiClient, config::NodeConfig, output::manifest};

#[derive(Clone)]
struct ResolvedTarget {
    static_dir: PathBuf,
    spa_fallback: bool,
    not_found: Option<String>,
}

struct CachedResolution {
    target: Option<ResolvedTarget>,
    fetched_at: Instant,
}

pub struct ServeState {
    client: ControlApiClient,
    cache_root: PathBuf,
    metadata_ttl: Duration,
    resolutions: Mutex<HashMap<String, CachedResolution>>,
}

impl ServeState {
    pub fn new(client: ControlApiClient, config: &NodeConfig) -> Self {
        Self {
            client,
            cache_root: PathBuf::from(&config.serve.artifact_cache_root),
            metadata_ttl: Duration::from_secs(config.serve.metadata_cache_ttl_seconds.max(1)),
            resolutions: Mutex::new(HashMap::new()),
        }
    }
}

pub fn spawn(state: Arc<ServeState>, config: &NodeConfig) -> tokio::task::JoinHandle<()> {
    let addr = std::net::SocketAddr::new(config.serve.host, config.serve.port);
    tokio::spawn(async move {
        let app = axum::Router::new()
            .fallback(handle_request)
            .with_state(state);
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                warn!(operation = "node.serve.bind", %error, %addr, "serve listener bind failed");
                return;
            }
        };
        info!(operation = "node.serve.start", %addr, "public serve listener started");
        if let Err(error) = axum::serve(listener, app).await {
            warn!(operation = "node.serve.stopped", %error, "serve listener stopped");
        }
    })
}

/// Normalizes a public request path into safe relative segments. Returns
/// `None` for anything that tries to escape the static root.
pub fn normalize_public_path(path: &str) -> Option<Vec<String>> {
    if path.contains('\0') || path.contains('\\') {
        return None;
    }
    // Percent-decode so encoded traversal (%2e%2e) cannot slip through.
    let decoded = percent_decode(path)?;
    if decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }

    let mut segments = Vec::new();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            segment => segments.push(segment.to_owned()),
        }
    }
    Some(segments)
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = bytes.get(index + 1)?;
                let low = bytes.get(index + 2)?;
                let value =
                    (char::from(*high).to_digit(16)? * 16 + char::from(*low).to_digit(16)?) as u8;
                output.push(value);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).ok()
}

/// Resolves a normalized request path against the static directory:
/// directories serve their `index.html`, missing paths fall back to the SPA
/// index when enabled.
pub fn resolve_static_file(
    static_dir: &Path,
    segments: &[String],
    spa_fallback: bool,
) -> Option<PathBuf> {
    let mut candidate = static_dir.to_path_buf();
    for segment in segments {
        candidate.push(segment);
    }

    if candidate.is_dir() {
        candidate.push("index.html");
    }
    if candidate.is_file() {
        return Some(candidate);
    }

    // Pretty URLs: /about → /about.html when present.
    if let Some(last) = segments.last()
        && !last.contains('.')
    {
        let mut with_extension = static_dir.to_path_buf();
        for segment in &segments[..segments.len() - 1] {
            with_extension.push(segment);
        }
        with_extension.push(format!("{last}.html"));
        if with_extension.is_file() {
            return Some(with_extension);
        }
    }

    if spa_fallback {
        let index = static_dir.join("index.html");
        if index.is_file() {
            return Some(index);
        }
    }
    None
}

fn host_from_headers(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::HOST)?.to_str().ok()?;
    let without_port = raw.rsplit_once(':').map_or(raw, |(host, port)| {
        if port.chars().all(|character| character.is_ascii_digit()) {
            host
        } else {
            raw
        }
    });
    grass_validator::normalize_host(without_port).ok()
}

async fn handle_request(
    State(state): State<Arc<ServeState>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(host) = host_from_headers(&headers) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "This request has no valid Host header.",
        );
    };

    let target = match resolve_host(&state, &host).await {
        Ok(Some(target)) => target,
        Ok(None) => {
            return error_page(
                StatusCode::NOT_FOUND,
                "This host is not bound to any active deployment.",
            );
        }
        Err(error) => {
            warn!(operation = "node.serve.resolve_host", %error, host = %host, "host resolution failed");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not be reached to resolve this host.",
            );
        }
    };

    let Some(segments) = normalize_public_path(uri.path()) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "The requested path is not allowed.",
        );
    };

    match resolve_static_file(&target.static_dir, &segments, target.spa_fallback) {
        Some(file) => serve_file(&file, StatusCode::OK).await,
        None => {
            if let Some(not_found) = &target.not_found {
                let mut custom = target.static_dir.clone();
                for segment in not_found.trim_start_matches('/').split('/') {
                    custom.push(segment);
                }
                if custom.is_file() {
                    return serve_file(&custom, StatusCode::NOT_FOUND).await;
                }
            }
            let fallback_404 = target.static_dir.join("404.html");
            if fallback_404.is_file() {
                return serve_file(&fallback_404, StatusCode::NOT_FOUND).await;
            }
            error_page(StatusCode::NOT_FOUND, "This page could not be found.")
        }
    }
}

async fn serve_file(path: &Path, status: StatusCode) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache_control = if mime == mime_guess::mime::TEXT_HTML {
                "public, max-age=0, must-revalidate"
            } else {
                "public, max-age=3600"
            };
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, cache_control)
                .body(Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => error_page(StatusCode::NOT_FOUND, "This page could not be found."),
    }
}

fn error_page(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{code}</title></head>\
         <body style=\"font-family:system-ui;margin:4rem auto;max-width:36rem;text-align:center\">\
         <h1>{code}</h1><p>{message}</p><p style=\"color:#888\">grass-worker node</p></body></html>",
        code = status.as_u16(),
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_else(|_| status.into_response())
}

async fn resolve_host(state: &ServeState, host: &str) -> anyhow::Result<Option<ResolvedTarget>> {
    {
        let cache = state.resolutions.lock().await;
        if let Some(entry) = cache.get(host)
            && entry.fetched_at.elapsed() < state.metadata_ttl
        {
            return Ok(entry.target.clone());
        }
    }

    let resolved = state.client.resolve_host(host).await?;
    let target = match resolved {
        Some(resolution) => match ensure_artifact(state, resolution.deployment_id).await {
            Ok(target) => Some(target),
            Err(error) => {
                warn!(
                    operation = "node.serve.artifact",
                    %error,
                    deployment_id = %resolution.deployment_id,
                    "artifact preparation failed"
                );
                None
            }
        },
        None => None,
    };

    let mut cache = state.resolutions.lock().await;
    cache.insert(
        host.to_owned(),
        CachedResolution {
            target: target.clone(),
            fetched_at: Instant::now(),
        },
    );
    Ok(target)
}

/// Ensures the deployment artifact is unpacked locally and returns the
/// serve target described by its manifest.
async fn ensure_artifact(
    state: &ServeState,
    deployment_id: Uuid,
) -> anyhow::Result<ResolvedTarget> {
    let deployment_dir = state.cache_root.join(deployment_id.to_string());
    let manifest_path = deployment_dir.join("output.toml");

    if !manifest_path.is_file() {
        let bytes = state
            .client
            .download_artifact(deployment_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("artifact is not available yet"))?;
        let unpack_dir = deployment_dir.clone();
        tokio::task::spawn_blocking(move || {
            if unpack_dir.exists() {
                let _ = std::fs::remove_dir_all(&unpack_dir);
            }
            grass_archive::unpack_zip_bytes(&bytes, &unpack_dir)
        })
        .await??;
    }

    let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
    let manifest = manifest::parse_manifest(&manifest_content)
        .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;
    manifest::validate_manifest(&manifest, &deployment_dir)
        .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;

    let static_section = manifest
        .static_site
        .ok_or_else(|| anyhow::anyhow!("manifest has no static section"))?;

    Ok(ResolvedTarget {
        static_dir: deployment_dir.join(static_section.directory),
        spa_fallback: static_section.spa_fallback,
        not_found: (!static_section.not_found.trim().is_empty())
            .then(|| static_section.not_found.trim().to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_site(spa: bool) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grass-serve-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("index.html"), "<html>index</html>").unwrap();
        std::fs::write(dir.join("about.html"), "<html>about</html>").unwrap();
        std::fs::write(dir.join("assets/app.js"), "js").unwrap();
        std::fs::write(dir.join("docs/index.html"), "<html>docs</html>").unwrap();
        let _ = spa;
        dir
    }

    #[test]
    fn public_paths_are_normalized_and_traversal_is_rejected() {
        assert_eq!(normalize_public_path("/"), Some(vec![]));
        assert_eq!(
            normalize_public_path("/assets/app.js"),
            Some(vec!["assets".to_owned(), "app.js".to_owned()])
        );
        assert_eq!(
            normalize_public_path("/a/./b"),
            Some(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(normalize_public_path("/../etc/passwd"), None);
        assert_eq!(normalize_public_path("/a/../../etc"), None);
        assert_eq!(normalize_public_path("/%2e%2e/secret"), None);
        assert_eq!(normalize_public_path("/a%2F..%2F..%2Fetc"), None);
        assert_eq!(normalize_public_path("/back\\slash"), None);
    }

    #[test]
    fn static_resolution_serves_index_pretty_urls_and_spa_fallback() {
        let dir = static_site(true);

        // Root and directory index.
        assert_eq!(
            resolve_static_file(&dir, &[], false).unwrap(),
            dir.join("index.html")
        );
        assert_eq!(
            resolve_static_file(&dir, &["docs".to_owned()], false).unwrap(),
            dir.join("docs/index.html")
        );

        // Direct file and pretty URL.
        assert_eq!(
            resolve_static_file(&dir, &["assets".to_owned(), "app.js".to_owned()], false).unwrap(),
            dir.join("assets/app.js")
        );
        assert_eq!(
            resolve_static_file(&dir, &["about".to_owned()], false).unwrap(),
            dir.join("about.html")
        );

        // SPA fallback on unknown routes only when enabled.
        assert_eq!(
            resolve_static_file(&dir, &["missing".to_owned()], true).unwrap(),
            dir.join("index.html")
        );
        assert_eq!(
            resolve_static_file(&dir, &["missing".to_owned()], false),
            None
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn host_header_parsing_strips_ports_and_normalizes() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "Demo.Grass.Test:8080".parse().unwrap());
        assert_eq!(
            host_from_headers(&headers).as_deref(),
            Some("demo.grass.test")
        );

        headers.insert(header::HOST, "demo.grass.test".parse().unwrap());
        assert_eq!(
            host_from_headers(&headers).as_deref(),
            Some("demo.grass.test")
        );

        headers.insert(header::HOST, "..".parse().unwrap());
        assert_eq!(host_from_headers(&headers), None);
    }
}
