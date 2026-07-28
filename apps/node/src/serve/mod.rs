//! Public serving.
//!
//! Every public request resolves its Host header through the Control API
//! (with a short-lived local cache) and locates the deployment's unpacked
//! Grass Output. Static outputs are served from the manifest's static
//! directory with index handling, SPA fallback, and strict path
//! normalization; SSR outputs are reverse-proxied to the deployment's
//! service container (started on demand by [`ssr::SsrManager`]).

pub mod ssr;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use grass_node_protocol::{ResolveHostResponse, ServeAccess};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    client::{ControlApiClient, PreviewAuthError},
    config::NodeConfig,
    output::manifest,
};

const PREVIEW_COOKIE: &str = "__Host-gw_preview_access";
const PREVIEW_CALLBACK_PATH: &str = "/.grass/auth/callback";

#[derive(Clone)]
enum ResolvedTarget {
    Static {
        static_dir: PathBuf,
        spa_fallback: bool,
        not_found: Option<String>,
    },
    Ssr {
        deployment_id: Uuid,
        deployment_dir: PathBuf,
        server: manifest::ServerSection,
    },
}

#[derive(Clone)]
struct ResolvedSite {
    resolution: ResolveHostResponse,
    target: ResolvedTarget,
}

struct CachedResolution {
    target: Option<ResolvedSite>,
    fetched_at: Instant,
}

pub struct ServeState {
    client: ControlApiClient,
    cache_root: PathBuf,
    metadata_ttl: Duration,
    resolutions: Mutex<HashMap<String, CachedResolution>>,
    preview_access_ttl: Duration,
    preview_grants: Mutex<HashMap<String, Instant>>,
    ssr: Arc<ssr::SsrManager>,
    /// Proxy client for SSR upstreams: connect timeout only, so streamed
    /// responses (SSE, long polls) are never cut off by a total timeout.
    proxy: reqwest::Client,
}

impl ServeState {
    pub fn new(client: ControlApiClient, config: &NodeConfig, ssr: Arc<ssr::SsrManager>) -> Self {
        Self {
            client,
            cache_root: PathBuf::from(&config.serve.artifact_cache_root),
            metadata_ttl: Duration::from_secs(config.serve.metadata_cache_ttl_seconds.max(1)),
            resolutions: Mutex::new(HashMap::new()),
            preview_access_ttl: Duration::from_secs(
                config.serve.metadata_cache_ttl_seconds.clamp(1, 30),
            ),
            preview_grants: Mutex::new(HashMap::new()),
            ssr,
            proxy: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("static reqwest options cannot fail"),
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
        if let Err(error) = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
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

fn resolve_not_found_file(static_dir: &Path, configured: Option<&str>) -> Option<PathBuf> {
    if let Some(configured) = configured {
        let mut candidate = static_dir.to_path_buf();
        for segment in configured.trim_start_matches('/').split('/') {
            candidate.push(segment);
        }
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let root_404 = static_dir.join("404.html");
    root_404.is_file().then_some(root_404)
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

fn is_preview_callback(path: &str) -> bool {
    path == PREVIEW_CALLBACK_PATH
}

fn request_destination(path_and_query: &str) -> String {
    if path_and_query.is_empty() {
        "/".to_owned()
    } else {
        path_and_query.to_owned()
    }
}

fn preview_cookie_value(cookie_header: &str) -> Option<&str> {
    cookie_header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == PREVIEW_COOKIE).then_some(value)
    })
}

fn strip_preview_cookie(cookie_header: &str) -> Option<String> {
    let cookies = cookie_header
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let (name, _) = pair.split_once('=')?;
            (name != PREVIEW_COOKIE).then_some(pair)
        })
        .collect::<Vec<_>>();
    (!cookies.is_empty()).then(|| cookies.join("; "))
}

fn preview_access_cookie(grant: &str, max_age_seconds: u64) -> String {
    format!(
        "{PREVIEW_COOKIE}={grant}; Path=/; Max-Age={max_age_seconds}; Secure; HttpOnly; SameSite=Lax"
    )
}

fn clear_preview_cookie() -> String {
    format!("{PREVIEW_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax")
}

fn preview_cache_key(host: &str, grant: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(host.as_bytes());
    digest.update([0]);
    digest.update(grant.as_bytes());
    hex::encode(digest.finalize())
}

fn callback_code(request: &Request) -> Option<String> {
    let mut codes = request
        .uri()
        .query()
        .into_iter()
        .flat_map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .filter(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned());
    let code = codes.next().filter(|code| !code.is_empty())?;
    codes.next().is_none().then_some(code)
}

fn redirect_response(location: &str, cookie: Option<String>) -> Response {
    let Ok(location) = HeaderValue::from_str(location) else {
        return error_page(
            StatusCode::BAD_GATEWAY,
            "The control plane returned an invalid authorization redirect.",
        );
    };
    let mut response = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::REFERRER_POLICY, "no-referrer");
    if let Some(cookie) = cookie {
        let Ok(cookie) = HeaderValue::from_str(&cookie) else {
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane returned an invalid preview grant.",
            );
        };
        response = response.header(header::SET_COOKIE, cookie);
    }
    response
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn begin_preview_authorization(
    state: &ServeState,
    host: &str,
    return_to: &str,
    clear_cookie: bool,
) -> Response {
    match state
        .client
        .start_preview_authorization(host, return_to)
        .await
    {
        Ok(started) => redirect_response(
            &started.authorization_url,
            clear_cookie.then(clear_preview_cookie),
        ),
        Err(error) => {
            warn!(
                operation = "node.serve.preview_authorize",
                %error,
                host = %host,
                "preview authorization could not start"
            );
            error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not authorize this preview.",
            )
        }
    }
}

async fn handle_preview_callback(state: &ServeState, host: &str, code: Option<String>) -> Response {
    let Some(code) = code else {
        return begin_preview_authorization(state, host, "/", true).await;
    };
    match state.client.exchange_preview_code(host, &code).await {
        Ok(exchanged) => redirect_response(
            &exchanged.return_to,
            Some(preview_access_cookie(
                &exchanged.grant,
                exchanged.max_age_seconds.min(12 * 60 * 60),
            )),
        ),
        Err(PreviewAuthError::Unauthorized) => {
            begin_preview_authorization(state, host, "/", true).await
        }
        Err(PreviewAuthError::Forbidden) => error_page(
            StatusCode::FORBIDDEN,
            "Your account is not a member of the team that owns this preview.",
        ),
        Err(PreviewAuthError::Infrastructure(error)) => {
            warn!(
                operation = "node.serve.preview_exchange",
                %error,
                host = %host,
                "preview callback exchange failed"
            );
            error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not complete preview authorization.",
            )
        }
    }
}

async fn require_preview_access(
    state: &ServeState,
    host: &str,
    destination: String,
    grant: Option<String>,
) -> Result<(), Response> {
    let Some(grant) = grant else {
        return Err(begin_preview_authorization(state, host, &destination, false).await);
    };

    let cache_key = preview_cache_key(host, &grant);
    {
        let mut grants = state.preview_grants.lock().await;
        if grants
            .get(&cache_key)
            .is_some_and(|expires_at| *expires_at > Instant::now())
        {
            return Ok(());
        }
        grants.remove(&cache_key);
    }

    match state.client.verify_preview_grant(host, &grant).await {
        Ok(verification) if verification.allowed => {
            let mut grants = state.preview_grants.lock().await;
            let now = Instant::now();
            grants.retain(|_, expires_at| *expires_at > now);
            grants.insert(cache_key, now + state.preview_access_ttl);
            Ok(())
        }
        Ok(_) | Err(PreviewAuthError::Forbidden) => Err(error_page(
            StatusCode::FORBIDDEN,
            "Your account is not a member of the team that owns this preview.",
        )),
        Err(PreviewAuthError::Unauthorized) => {
            Err(begin_preview_authorization(state, host, &destination, true).await)
        }
        Err(PreviewAuthError::Infrastructure(error)) => {
            warn!(
                operation = "node.serve.preview_verify",
                %error,
                host = %host,
                "preview grant verification failed"
            );
            Err(error_page(
                StatusCode::BAD_GATEWAY,
                "The control plane could not verify preview access.",
            ))
        }
    }
}

fn strip_preview_cookie_header(headers: &mut HeaderMap) {
    let filtered = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(strip_preview_cookie);
    match filtered.and_then(|value| HeaderValue::from_str(&value).ok()) {
        Some(value) => {
            headers.insert(header::COOKIE, value);
        }
        None => {
            headers.remove(header::COOKIE);
        }
    }
}

async fn handle_request(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    mut request: Request,
) -> Response {
    let Some(host) = host_from_headers(request.headers()) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "This request has no valid Host header.",
        );
    };

    let site = match resolve_host(&state, &host).await {
        Ok(Some(site)) => site,
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

    let requires_preview_access = matches!(site.resolution.access, ServeAccess::TeamMember);
    if requires_preview_access {
        if is_preview_callback(request.uri().path()) {
            let code = callback_code(&request);
            return handle_preview_callback(&state, &host, code).await;
        }
        let destination = request_destination(
            request
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/"),
        );
        let grant = request
            .headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(preview_cookie_value)
            .map(str::to_owned);
        if let Err(response) = require_preview_access(&state, &host, destination, grant).await {
            return response;
        }
    }

    match site.target {
        ResolvedTarget::Static {
            static_dir,
            spa_fallback,
            not_found,
        } => {
            let Some(segments) = normalize_public_path(request.uri().path()) else {
                return error_page(
                    StatusCode::BAD_REQUEST,
                    "The requested path is not allowed.",
                );
            };

            match resolve_static_file(&static_dir, &segments, spa_fallback) {
                Some(file) => serve_file(&file, StatusCode::OK).await,
                None => {
                    if let Some(not_found_file) =
                        resolve_not_found_file(&static_dir, not_found.as_deref())
                    {
                        return serve_file(&not_found_file, StatusCode::NOT_FOUND).await;
                    }
                    error_page(StatusCode::NOT_FOUND, "This page could not be found.")
                }
            }
        }
        ResolvedTarget::Ssr {
            deployment_id,
            deployment_dir,
            server,
        } => {
            if requires_preview_access {
                strip_preview_cookie_header(request.headers_mut());
            }
            let upstream = match state
                .ssr
                .upstream_for(deployment_id, &deployment_dir, &server)
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => {
                    warn!(
                        operation = "node.serve.ssr_start",
                        %error,
                        deployment_id = %deployment_id,
                        "ssr service unavailable"
                    );
                    return error_page(
                        StatusCode::BAD_GATEWAY,
                        "The application server failed to start.",
                    );
                }
            };
            match forward_to_ssr(&state.proxy, &upstream, client_addr, request).await {
                Ok(response) => response,
                Err(error) => {
                    // A connect failure means the container died or lost its
                    // address; drop it so the next request restarts it.
                    if error.is_connect() {
                        state.ssr.invalidate(deployment_id).await;
                    }
                    warn!(
                        operation = "node.serve.ssr_proxy",
                        %error,
                        deployment_id = %deployment_id,
                        "ssr proxy request failed"
                    );
                    error_page(
                        StatusCode::BAD_GATEWAY,
                        "The application server could not be reached.",
                    )
                }
            }
        }
    }
}

/// Hop-by-hop headers never forwarded in either direction.
fn is_hop_by_hop(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

/// Streams the request to the SSR upstream and the response back, keeping
/// end-to-end headers and adding the standard forwarding metadata.
async fn forward_to_ssr(
    proxy: &reqwest::Client,
    upstream: &str,
    client_addr: SocketAddr,
    request: Request,
) -> Result<Response, reqwest::Error> {
    let (parts, body) = request.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let url = format!("http://{upstream}{path_and_query}");

    let mut builder = proxy.request(parts.method.clone(), url);
    for (name, value) in &parts.headers {
        if is_hop_by_hop(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder
        .header("x-forwarded-proto", "http")
        .header("x-forwarded-for", client_addr.ip().to_string());
    if let Some(host) = parts.headers.get(header::HOST) {
        builder = builder.header("x-forwarded-host", host);
    }

    let upstream_response = builder
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await?;

    let mut response = Response::builder().status(upstream_response.status());
    for (name, value) in upstream_response.headers() {
        if is_hop_by_hop(name) {
            continue;
        }
        response = response.header(name, value);
    }
    Ok(response
        .body(Body::from_stream(upstream_response.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()))
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
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(Body::from(body))
        .unwrap_or_else(|_| status.into_response())
}

async fn resolve_host(state: &ServeState, host: &str) -> anyhow::Result<Option<ResolvedSite>> {
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
            Ok(target) => Some(ResolvedSite { resolution, target }),
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

    if manifest.runtime.kind == "ssr" {
        let server = manifest
            .server
            .ok_or_else(|| anyhow::anyhow!("ssr manifest has no server section"))?;
        return Ok(ResolvedTarget::Ssr {
            deployment_id,
            deployment_dir,
            server,
        });
    }

    let static_section = manifest
        .static_site
        .ok_or_else(|| anyhow::anyhow!("manifest has no static section"))?;

    Ok(ResolvedTarget::Static {
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
    fn missing_static_paths_select_custom_then_root_404() {
        let dir = static_site(false);
        std::fs::create_dir_all(dir.join("errors")).unwrap();
        std::fs::write(dir.join("errors/not-found.html"), "custom").unwrap();
        std::fs::write(dir.join("404.html"), "root").unwrap();

        assert_eq!(
            resolve_not_found_file(&dir, Some("errors/not-found.html")),
            Some(dir.join("errors/not-found.html"))
        );
        assert_eq!(
            resolve_not_found_file(&dir, Some("missing.html")),
            Some(dir.join("404.html"))
        );

        std::fs::remove_file(dir.join("404.html")).unwrap();
        assert_eq!(resolve_not_found_file(&dir, None), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preview_cookie_contract_and_ssr_filtering_are_host_scoped() {
        assert_eq!(
            preview_access_cookie("opaque", 43_200),
            "__Host-gw_preview_access=opaque; Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            preview_cookie_value("app=1; __Host-gw_preview_access=opaque; theme=dark"),
            Some("opaque")
        );
        assert_eq!(
            strip_preview_cookie("app=1; __Host-gw_preview_access=opaque; theme=dark"),
            Some("app=1; theme=dark".to_owned())
        );
        assert_eq!(
            strip_preview_cookie("__Host-gw_preview_access=opaque"),
            None
        );
    }

    #[test]
    fn preview_callback_is_reserved_and_destinations_keep_the_query() {
        assert!(is_preview_callback("/.grass/auth/callback"));
        assert!(!is_preview_callback("/.grass/auth/callback/child"));
        assert_eq!(request_destination("/docs?q=1"), "/docs?q=1");
        assert_eq!(request_destination(""), "/");
    }

    #[test]
    fn platform_error_pages_do_not_send_authorization_urls_as_referrers() {
        let response = error_page(StatusCode::BAD_GATEWAY, "unavailable");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
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
