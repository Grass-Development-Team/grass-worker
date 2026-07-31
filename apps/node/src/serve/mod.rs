//! Public serving.
//!
//! Every public request resolves its Host header through the last valid local
//! route snapshot. Local assignments serve their staged Grass Output;
//! requests assigned elsewhere make one authenticated peer hop. Static
//! outputs use strict path normalization, while SSR outputs are proxied to a
//! deployment service container started on demand by [`ssr::SsrManager`].

pub mod routes;
pub mod ssr;
pub mod static_files;
pub mod sync;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, post},
};
use grass_node_protocol::{ServeAccess, ServeRoute};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    client::{ControlApiClient, PreviewAuthError},
    config::NodeConfig,
    output::manifest,
};

const SECURE_PREVIEW_COOKIE: &str = "__Host-gw_preview_access";
const INSECURE_PREVIEW_COOKIE: &str = "gw_preview_access";
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

pub struct ServeState {
    client: ControlApiClient,
    node_id: Uuid,
    gateway_token: String,
    routes: Arc<routes::RouteTable>,
    cache_root: PathBuf,
    targets: Mutex<HashMap<Uuid, ResolvedTarget>>,
    preview_access_ttl: Duration,
    preview_grants: Mutex<HashMap<String, Instant>>,
    ssr: Arc<ssr::SsrManager>,
    /// Proxy client for peer Nodes and SSR upstreams: connect timeout only,
    /// so streamed responses are never cut off by a total timeout.
    proxy: reqwest::Client,
}

impl ServeState {
    pub fn new(
        client: ControlApiClient,
        node_id: Uuid,
        gateway_token: String,
        routes: Arc<routes::RouteTable>,
        config: &NodeConfig,
        ssr: Arc<ssr::SsrManager>,
    ) -> Self {
        Self {
            client,
            node_id,
            gateway_token,
            routes,
            cache_root: PathBuf::from(&config.serve.artifact_cache_root),
            targets: Mutex::new(HashMap::new()),
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

fn serve_router(state: Arc<ServeState>) -> axum::Router {
    axum::Router::new()
        .route(ROUTE_INVALIDATION_PATH, post(invalidate_routes))
        .route(PEER_PROXY_PREFIX, any(handle_peer_proxy))
        .route("/_grass/internal/proxy/{*path}", any(handle_peer_proxy))
        .fallback(route_public_request)
        .with_state(state)
}

#[derive(Deserialize)]
struct RouteInvalidationRequest {
    deployment_id: Uuid,
}

async fn invalidate_routes(
    State(state): State<Arc<ServeState>>,
    headers: HeaderMap,
    Json(body): Json<RouteInvalidationRequest>,
) -> Response {
    let authenticated = headers
        .get(GATEWAY_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|token| bool::from(token.as_bytes().ct_eq(state.gateway_token.as_bytes())));
    if !authenticated {
        return StatusCode::FORBIDDEN.into_response();
    }

    let removed = state.routes.remove_deployment(body.deployment_id).await;
    let routed_here = state.routes.local_deployment_ids(state.node_id).await;
    let routed_anywhere = state.routes.deployment_ids().await;
    if let Err(error) = state
        .ssr
        .reconcile_routes(&routed_here, &routed_anywhere)
        .await
    {
        warn!(
            operation = "node.serve.routes.invalidation_reconcile_failed",
            deployment_id = %body.deployment_id,
            %error,
            "route invalidation succeeded but SSR reconciliation failed"
        );
    }

    Json(serde_json::json!({
        "acknowledged": true,
        "removed": removed,
    }))
    .into_response()
}

pub fn spawn(state: Arc<ServeState>, config: &NodeConfig) -> tokio::task::JoinHandle<()> {
    let addr = std::net::SocketAddr::new(config.serve.host, config.serve.port);
    tokio::spawn(async move {
        let app = serve_router(state);
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

const GATEWAY_TOKEN_HEADER: &str = "x-grass-gateway-token";
const GATEWAY_HOP_HEADER: &str = "x-grass-gateway-hop";
const PEER_PROXY_PREFIX: &str = "/_grass/internal/proxy";
const ROUTE_INVALIDATION_PATH: &str = "/_grass/internal/routes/invalidate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayOrigin {
    External,
    Authenticated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteAction {
    Local,
    Proxy,
}

fn gateway_origin(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<GatewayOrigin, &'static str> {
    let token = headers.get(GATEWAY_TOKEN_HEADER);
    let hop = headers.get(GATEWAY_HOP_HEADER);
    match (token, hop) {
        (None, None) => Ok(GatewayOrigin::External),
        (Some(token), Some(hop)) => {
            let token = token.to_str().map_err(|_| "invalid gateway token")?;
            let hop = hop.to_str().map_err(|_| "invalid gateway hop")?;
            let valid_token: bool = token.as_bytes().ct_eq(expected_token.as_bytes()).into();
            if !valid_token {
                return Err("invalid gateway token");
            }
            if hop != "1" {
                return Err("invalid gateway hop");
            }
            Ok(GatewayOrigin::Authenticated)
        }
        _ => Err("incomplete gateway authentication"),
    }
}

fn route_action(
    local_node_id: Uuid,
    target_node_id: Uuid,
    origin: GatewayOrigin,
) -> Result<RouteAction, &'static str> {
    if local_node_id == target_node_id {
        return Ok(RouteAction::Local);
    }
    if matches!(origin, GatewayOrigin::Authenticated) {
        return Err("gateway request cannot be proxied more than once");
    }
    Ok(RouteAction::Proxy)
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
    [SECURE_PREVIEW_COOKIE, INSECURE_PREVIEW_COOKIE]
        .into_iter()
        .find_map(|expected| {
            cookie_header.split(';').find_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (name == expected).then_some(value)
            })
        })
}

fn strip_preview_cookie(cookie_header: &str) -> Option<String> {
    let cookies = cookie_header
        .split(';')
        .filter_map(|pair| {
            let pair = pair.trim();
            let (name, _) = pair.split_once('=')?;
            (![SECURE_PREVIEW_COOKIE, INSECURE_PREVIEW_COOKIE].contains(&name)).then_some(pair)
        })
        .collect::<Vec<_>>();
    (!cookies.is_empty()).then(|| cookies.join("; "))
}

fn preview_access_cookie(grant: &str, max_age_seconds: u64, secure: bool) -> String {
    let (name, secure_attribute) = if secure {
        (SECURE_PREVIEW_COOKIE, "; Secure")
    } else {
        (INSECURE_PREVIEW_COOKIE, "")
    };
    format!(
        "{name}={grant}; Path=/; Max-Age={max_age_seconds}{secure_attribute}; HttpOnly; SameSite=Lax"
    )
}

fn clear_preview_cookies() -> Vec<String> {
    vec![
        format!("{SECURE_PREVIEW_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"),
        format!("{INSECURE_PREVIEW_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
    ]
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

fn redirect_response(location: &str, cookies: Vec<String>) -> Response {
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
    for cookie in cookies {
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
            clear_cookie.then(clear_preview_cookies).unwrap_or_default(),
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
            vec![preview_access_cookie(
                &exchanged.grant,
                exchanged.max_age_seconds.min(12 * 60 * 60),
                exchanged.cookie_secure,
            )],
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

fn strip_peer_proxy_prefix(request: &mut Request) -> Result<(), &'static str> {
    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let suffix = path_and_query
        .strip_prefix(PEER_PROXY_PREFIX)
        .ok_or("missing peer proxy prefix")?;
    let restored = match suffix.chars().next() {
        None => "/".to_owned(),
        Some('/') => suffix.to_owned(),
        Some('?') => format!("/{suffix}"),
        Some(_) => return Err("invalid peer proxy path"),
    };
    *request.uri_mut() = restored.parse().map_err(|_| "invalid peer proxy URI")?;
    Ok(())
}

async fn route_public_request(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let Some(host) = host_from_headers(request.headers()) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "This request has no valid Host header.",
        );
    };

    let Some(route) = state.routes.lookup(&host).await else {
        return error_page(
            StatusCode::NOT_FOUND,
            "This host is not bound to any active deployment.",
        );
    };
    match route_action(state.node_id, route.target_node_id, GatewayOrigin::External) {
        Ok(RouteAction::Proxy) => {
            return match forward_to_gateway(
                &state.proxy,
                &route.target_base_url,
                &state.gateway_token,
                client_addr,
                request,
            )
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    warn!(
                        operation = "node.serve.gateway",
                        %error,
                        host = %host,
                        target_node_id = %route.target_node_id,
                        "Serve gateway proxy failed"
                    );
                    error_page(
                        StatusCode::BAD_GATEWAY,
                        "The assigned Serve Node could not be reached.",
                    )
                }
            };
        }
        Ok(RouteAction::Local) => {}
        Err(_) => unreachable!("external requests can always proxy once"),
    }

    serve_local(state, route, client_addr, GatewayOrigin::External, request).await
}

async fn handle_peer_proxy(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    mut request: Request,
) -> Response {
    if !matches!(
        gateway_origin(request.headers(), &state.gateway_token),
        Ok(GatewayOrigin::Authenticated)
    ) {
        return error_page(
            StatusCode::FORBIDDEN,
            "This gateway request is not authorized.",
        );
    }
    if strip_peer_proxy_prefix(&mut request).is_err() {
        return error_page(StatusCode::BAD_REQUEST, "This gateway path is invalid.");
    }
    let Some(host) = host_from_headers(request.headers()) else {
        return error_page(
            StatusCode::BAD_REQUEST,
            "This request has no valid Host header.",
        );
    };
    let Some(route) = state.routes.lookup(&host).await else {
        return error_page(
            StatusCode::BAD_GATEWAY,
            "The gateway route snapshot no longer contains this Host.",
        );
    };
    if !matches!(
        route_action(
            state.node_id,
            route.target_node_id,
            GatewayOrigin::Authenticated,
        ),
        Ok(RouteAction::Local)
    ) {
        return error_page(
            StatusCode::BAD_GATEWAY,
            "The gateway route snapshot points to another Serve Node.",
        );
    }

    serve_local(
        state,
        route,
        client_addr,
        GatewayOrigin::Authenticated,
        request,
    )
    .await
}

async fn serve_local(
    state: Arc<ServeState>,
    route: ServeRoute,
    client_addr: SocketAddr,
    origin: GatewayOrigin,
    mut request: Request,
) -> Response {
    request.headers_mut().remove(GATEWAY_TOKEN_HEADER);
    request.headers_mut().remove(GATEWAY_HOP_HEADER);

    let target = match resolve_deployment(&state, route.deployment_id).await {
        Ok(target) => target,
        Err(error) => {
            warn!(operation = "node.serve.resolve_host", %error, host = %route.host, "local deployment resolution failed");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The assigned deployment is not ready on this Serve Node.",
            );
        }
    };

    let requires_preview_access = matches!(route.access, ServeAccess::TeamOrPlatformAdmin);
    if requires_preview_access {
        if is_preview_callback(request.uri().path()) {
            let code = callback_code(&request);
            return handle_preview_callback(&state, &route.host, code).await;
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
        if let Err(response) = require_preview_access(&state, &route.host, destination, grant).await
        {
            return response;
        }
    }

    match target {
        ResolvedTarget::Static {
            static_dir,
            spa_fallback,
            not_found,
        } => {
            let method = request.method().clone();
            let range = request.headers().get(header::RANGE).cloned();
            let Some(segments) = normalize_public_path(request.uri().path()) else {
                return error_page(
                    StatusCode::BAD_REQUEST,
                    "The requested path is not allowed.",
                );
            };

            match resolve_static_file(&static_dir, &segments, spa_fallback) {
                Some(file) => serve_file(&file, StatusCode::OK, &method, range.as_ref()).await,
                None => {
                    if let Some(not_found_file) =
                        resolve_not_found_file(&static_dir, not_found.as_deref())
                    {
                        return serve_file(
                            &not_found_file,
                            StatusCode::NOT_FOUND,
                            &method,
                            range.as_ref(),
                        )
                        .await;
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
                .upstream_for(deployment_id, &deployment_dir, &server, route.resources)
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
            match forward_to_ssr(&state.proxy, &upstream, client_addr, origin, request).await {
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

fn is_gateway_internal(name: &HeaderName) -> bool {
    matches!(name.as_str(), GATEWAY_TOKEN_HEADER | GATEWAY_HOP_HEADER)
}

fn is_forwarded_metadata(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "x-forwarded-for" | "x-forwarded-host" | "x-forwarded-proto"
    )
}

/// Streams the request to the SSR upstream and the response back, keeping
/// end-to-end headers and adding the standard forwarding metadata.
async fn forward_to_ssr(
    proxy: &reqwest::Client,
    upstream: &str,
    client_addr: SocketAddr,
    origin: GatewayOrigin,
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
        if is_hop_by_hop(name)
            || is_gateway_internal(name)
            || matches!(origin, GatewayOrigin::External) && is_forwarded_metadata(name)
        {
            continue;
        }
        builder = builder.header(name, value);
    }
    if matches!(origin, GatewayOrigin::External) {
        builder = builder
            .header("x-forwarded-proto", "http")
            .header("x-forwarded-for", client_addr.ip().to_string());
        if let Some(host) = parts.headers.get(header::HOST) {
            builder = builder.header("x-forwarded-host", host);
        }
    }

    let upstream_response = builder
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await?;

    let mut response = Response::builder().status(upstream_response.status());
    for (name, value) in upstream_response.headers() {
        if is_hop_by_hop(name) || is_gateway_internal(name) {
            continue;
        }
        response = response.header(name, value);
    }
    Ok(response
        .body(Body::from_stream(upstream_response.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()))
}

async fn forward_to_gateway(
    proxy: &reqwest::Client,
    target_base_url: &str,
    gateway_token: &str,
    client_addr: SocketAddr,
    request: Request,
) -> anyhow::Result<Response> {
    let (parts, body) = request.into_parts();
    let mut url = url::Url::parse(target_base_url)
        .map_err(|error| anyhow::anyhow!("invalid target Serve Node URL: {error}"))?;
    url.set_path(&format!("{PEER_PROXY_PREFIX}{}", parts.uri.path()));
    url.set_query(parts.uri.query());

    let mut builder = proxy.request(parts.method, url);
    for (name, value) in &parts.headers {
        if is_hop_by_hop(name) || is_gateway_internal(name) || is_forwarded_metadata(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder
        .header(GATEWAY_TOKEN_HEADER, gateway_token)
        .header(GATEWAY_HOP_HEADER, "1")
        .header("x-forwarded-for", client_addr.ip().to_string())
        .header("x-forwarded-proto", "http");
    if let Some(host) = parts.headers.get(header::HOST) {
        builder = builder.header("x-forwarded-host", host);
    }
    let upstream = builder
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await?;

    let mut response = Response::builder().status(upstream.status());
    for (name, value) in upstream.headers() {
        if !is_hop_by_hop(name) && !is_gateway_internal(name) {
            response = response.header(name, value);
        }
    }
    Ok(response
        .body(Body::from_stream(upstream.bytes_stream()))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()))
}

async fn serve_file(
    path: &Path,
    status: StatusCode,
    method: &axum::http::Method,
    range: Option<&axum::http::HeaderValue>,
) -> Response {
    match static_files::serve_file(path, method, range, status).await {
        Ok(response) => response,
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

async fn resolve_deployment(
    state: &ServeState,
    deployment_id: Uuid,
) -> anyhow::Result<ResolvedTarget> {
    if let Some(target) = state.targets.lock().await.get(&deployment_id).cloned() {
        return Ok(target);
    }
    let target = ensure_artifact(state, deployment_id).await?;
    state
        .targets
        .lock()
        .await
        .insert(deployment_id, target.clone());
    Ok(target)
}

/// Loads the already staged deployment artifact and returns the serve target
/// described by its manifest.
async fn ensure_artifact(
    state: &ServeState,
    deployment_id: Uuid,
) -> anyhow::Result<ResolvedTarget> {
    let deployment_dir = sync::staged_artifact_path(&state.cache_root, deployment_id)?;
    let manifest_path = deployment_dir.join("output.toml");

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
            preview_access_cookie("opaque", 43_200, true),
            "__Host-gw_preview_access=opaque; Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            preview_access_cookie("opaque", 43_200, false),
            "gw_preview_access=opaque; Path=/; Max-Age=43200; HttpOnly; SameSite=Lax"
        );
        assert_eq!(
            preview_cookie_value(
                "app=1; __Host-gw_preview_access=secure; gw_preview_access=plain; theme=dark"
            ),
            Some("secure")
        );
        assert_eq!(
            preview_cookie_value("app=1; gw_preview_access=plain; theme=dark"),
            Some("plain")
        );
        assert_eq!(
            strip_preview_cookie(
                "app=1; __Host-gw_preview_access=secure; gw_preview_access=plain; theme=dark"
            ),
            Some("app=1; theme=dark".to_owned())
        );
        assert_eq!(
            strip_preview_cookie("__Host-gw_preview_access=opaque"),
            None
        );
        assert_eq!(
            clear_preview_cookies(),
            vec![
                "__Host-gw_preview_access=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
                    .to_owned(),
                "gw_preview_access=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".to_owned(),
            ]
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
    fn preview_redirect_can_clear_secure_and_http_development_cookies() {
        let response = redirect_response("/", clear_preview_cookies());
        assert_eq!(
            response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .count(),
            2
        );
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

    #[test]
    fn gateway_hops_authenticate_and_never_reproxy() {
        let token = "shared-gateway-token";
        let mut headers = HeaderMap::new();
        let external = gateway_origin(&headers, token).unwrap();
        assert_eq!(external, GatewayOrigin::External);
        assert_eq!(
            route_action(Uuid::nil(), Uuid::now_v7(), external).unwrap(),
            RouteAction::Proxy
        );

        headers.insert("x-grass-gateway-token", token.parse().unwrap());
        headers.insert("x-grass-gateway-hop", "1".parse().unwrap());
        let authenticated = gateway_origin(&headers, token).unwrap();
        assert_eq!(authenticated, GatewayOrigin::Authenticated);
        assert!(route_action(Uuid::nil(), Uuid::now_v7(), authenticated).is_err());

        headers.insert("x-grass-gateway-token", "wrong-token".parse().unwrap());
        assert!(gateway_origin(&headers, token).is_err());
        headers.insert("x-grass-gateway-token", token.parse().unwrap());
        headers.insert("x-grass-gateway-hop", "2".parse().unwrap());
        assert!(gateway_origin(&headers, token).is_err());
    }

    #[test]
    fn peer_proxy_prefix_is_removed_without_changing_path_or_query() {
        let mut request = Request::builder()
            .uri("/_grass/internal/proxy/submit/item?preview=1")
            .body(Body::empty())
            .unwrap();

        strip_peer_proxy_prefix(&mut request).unwrap();

        assert_eq!(
            request.uri().path_and_query().unwrap().as_str(),
            "/submit/item?preview=1"
        );
    }

    #[tokio::test]
    async fn gateway_proxy_preserves_request_and_adds_single_hop_auth() {
        let app = axum::Router::new().fallback(|request: Request| async move {
            assert_eq!(request.method(), axum::http::Method::POST);
            assert_eq!(
                request.uri().path_and_query().unwrap().as_str(),
                "/_grass/internal/proxy/submit/%2Fitem?preview=1"
            );
            assert_eq!(request.headers()[header::HOST], "app.example.com");
            assert_eq!(request.headers()[header::AUTHORIZATION], "Bearer app-token");
            assert_eq!(
                request.headers()[GATEWAY_TOKEN_HEADER],
                "shared-gateway-token"
            );
            assert_eq!(request.headers()[GATEWAY_HOP_HEADER], "1");
            assert_eq!(request.headers()["x-forwarded-for"], "192.0.2.10");
            assert_eq!(request.headers()["x-forwarded-host"], "app.example.com");
            assert_eq!(request.headers()["x-forwarded-proto"], "http");
            let body = axum::body::to_bytes(request.into_body(), 1024)
                .await
                .unwrap();
            assert_eq!(body, "payload");
            Response::new(Body::from("proxied"))
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let proxy = reqwest::Client::new();
        let request = Request::builder()
            .method("POST")
            .uri("/submit/%2Fitem?preview=1")
            .header(header::HOST, "app.example.com")
            .header(header::AUTHORIZATION, "Bearer app-token")
            .header("x-forwarded-for", "203.0.113.99")
            .header("x-forwarded-host", "spoofed.example.com")
            .header("x-forwarded-proto", "https")
            .body(Body::from("payload"))
            .unwrap();

        let response = forward_to_gateway(
            &proxy,
            &format!("http://{address}"),
            "shared-gateway-token",
            "192.0.2.10:43123".parse().unwrap(),
            request,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(body, "proxied");
        server.abort();
    }

    #[tokio::test]
    async fn ssr_proxy_sanitizes_external_headers_and_preserves_gateway_metadata() {
        let app = axum::Router::new().fallback(|request: Request| async move {
            let headers = request.headers();
            assert_eq!(headers[header::AUTHORIZATION], "Bearer app-token");
            assert!(!headers.contains_key(GATEWAY_TOKEN_HEADER));
            assert!(!headers.contains_key(GATEWAY_HOP_HEADER));
            match request.uri().path() {
                "/external" => {
                    assert_eq!(headers["x-forwarded-for"], "192.0.2.20");
                    assert_eq!(headers["x-forwarded-host"], "app.example.com");
                    assert_eq!(headers["x-forwarded-proto"], "http");
                }
                "/peer" => {
                    assert_eq!(headers["x-forwarded-for"], "198.51.100.40");
                    assert_eq!(headers["x-forwarded-host"], "app.example.com");
                    assert_eq!(headers["x-forwarded-proto"], "https");
                }
                path => panic!("unexpected SSR test path: {path}"),
            }
            Response::new(Body::empty())
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let proxy = reqwest::Client::new();

        let external = Request::builder()
            .uri("/external")
            .header(header::HOST, "app.example.com")
            .header(header::AUTHORIZATION, "Bearer app-token")
            .header(GATEWAY_TOKEN_HEADER, "must-not-leak")
            .header(GATEWAY_HOP_HEADER, "1")
            .header("x-forwarded-for", "203.0.113.99")
            .header("x-forwarded-host", "spoofed.example.com")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        forward_to_ssr(
            &proxy,
            &address.to_string(),
            "192.0.2.20:41234".parse().unwrap(),
            GatewayOrigin::External,
            external,
        )
        .await
        .unwrap();

        let peer = Request::builder()
            .uri("/peer")
            .header(header::HOST, "app.example.com")
            .header(header::AUTHORIZATION, "Bearer app-token")
            .header(GATEWAY_TOKEN_HEADER, "must-not-leak")
            .header(GATEWAY_HOP_HEADER, "1")
            .header("x-forwarded-for", "198.51.100.40")
            .header("x-forwarded-host", "app.example.com")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        forward_to_ssr(
            &proxy,
            &address.to_string(),
            "127.0.0.1:51234".parse().unwrap(),
            GatewayOrigin::Authenticated,
            peer,
        )
        .await
        .unwrap();

        server.abort();
    }

    #[tokio::test]
    async fn route_invalidation_is_authenticated_and_removes_cached_access_before_acknowledging() {
        let authority = axum::Router::new().fallback(|| async { "stale deployment" });
        let authority_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let authority_address = authority_listener.local_addr().unwrap();
        let authority_server =
            tokio::spawn(async move { axum::serve(authority_listener, authority).await.unwrap() });

        let config = NodeConfig::default();
        let node_id = Uuid::now_v7();
        let deployment_id = Uuid::now_v7();
        let routes = Arc::new(routes::RouteTable::default());
        routes
            .apply(grass_node_protocol::RouteSnapshotResponse {
                revision: "before-withdrawal".to_owned(),
                routes: vec![ServeRoute {
                    host: "app.example.com".to_owned(),
                    deployment_id,
                    target_node_id: Uuid::now_v7(),
                    target_base_url: format!("http://{authority_address}"),
                    resources: grass_node_protocol::ServeResources {
                        cpu_millicores: 50,
                        memory_mb: 64,
                        disk_mb: 256,
                    },
                    access: ServeAccess::Public,
                }],
            })
            .await
            .unwrap();
        let ssr = Arc::new(ssr::SsrManager::new(None, node_id, &config));
        let state = Arc::new(ServeState::new(
            ControlApiClient::new(&format!("http://{authority_address}"), "node-token").unwrap(),
            node_id,
            "shared-gateway-token".to_owned(),
            routes,
            &config,
            ssr,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                serve_router(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap()
        });

        let client = reqwest::Client::new();
        let rejected = client
            .post(format!(
                "http://{address}/_grass/internal/routes/invalidate"
            ))
            .header(GATEWAY_TOKEN_HEADER, "wrong-token")
            .json(&serde_json::json!({ "deployment_id": deployment_id }))
            .send()
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

        let stale = client
            .get(format!("http://{address}/"))
            .header(header::HOST, "app.example.com")
            .send()
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::OK);

        let invalidation = client
            .post(format!(
                "http://{address}/_grass/internal/routes/invalidate"
            ))
            .header(GATEWAY_TOKEN_HEADER, "shared-gateway-token")
            .json(&serde_json::json!({ "deployment_id": deployment_id }))
            .send()
            .await
            .unwrap();
        assert_eq!(invalidation.status(), StatusCode::OK);

        let response = client
            .get(format!("http://{address}/"))
            .header(header::HOST, "app.example.com")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        server.abort();
        authority_server.abort();
    }

    #[tokio::test]
    async fn peer_endpoint_requires_gateway_auth_before_route_lookup() {
        let config = NodeConfig::default();
        let routes = Arc::new(routes::RouteTable::default());
        let ssr = Arc::new(ssr::SsrManager::new(None, Uuid::now_v7(), &config));
        let state = Arc::new(ServeState::new(
            ControlApiClient::new("http://127.0.0.1:9", "node-token").unwrap(),
            Uuid::now_v7(),
            "shared-gateway-token".to_owned(),
            routes,
            &config,
            ssr,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                serve_router(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap()
        });
        let client = reqwest::Client::new();
        let endpoint = format!("http://{address}{PEER_PROXY_PREFIX}/path");

        let missing = client
            .get(&endpoint)
            .header(header::HOST, "app.example.com")
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::FORBIDDEN);

        let wrong = client
            .get(&endpoint)
            .header(header::HOST, "app.example.com")
            .header(GATEWAY_TOKEN_HEADER, "wrong-token")
            .header(GATEWAY_HOP_HEADER, "1")
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::FORBIDDEN);

        let authorized = client
            .get(&endpoint)
            .header(header::HOST, "app.example.com")
            .header(GATEWAY_TOKEN_HEADER, "shared-gateway-token")
            .header(GATEWAY_HOP_HEADER, "1")
            .send()
            .await
            .unwrap();
        assert_eq!(authorized.status(), StatusCode::BAD_GATEWAY);

        server.abort();
    }
}
