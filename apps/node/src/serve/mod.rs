//! Public serving.
//!
//! Every public request resolves its Host header through the last valid local
//! route snapshot. Local assignments serve their staged Grass Output;
//! requests assigned elsewhere make one authenticated peer hop. Static
//! outputs use strict path normalization, while SSR outputs are proxied to a
//! deployment service container started on demand by [`ssr::SsrManager`].

pub mod certificates;
pub mod routes;
pub mod ssr;
pub mod static_files;
pub mod sync;
pub mod tls;

mod local;
mod paths;
mod preview;
mod proxy;
mod response;
mod routing;

pub use paths::{normalize_public_path, resolve_static_file};

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use grass_node_protocol::GatewayAuthenticationMode;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{client::ControlApiClient, config::NodeConfig};
use local::ResolvedTarget;
use routing::{
    GatewayOrigin, PEER_PROXY_PREFIX, ROUTE_INVALIDATION_PATH, gateway_origin, handle_peer_proxy,
    host_from_headers, route_public_request,
};

pub struct ServeState {
    client: ControlApiClient,
    node_id: Uuid,
    gateway_token: Option<String>,
    gateway_authentication: GatewayAuthenticationMode,
    routes: Arc<routes::RouteTable>,
    cache_root: PathBuf,
    targets: Mutex<HashMap<Uuid, ResolvedTarget>>,
    preview_access_ttl: Duration,
    preview_grants: Mutex<HashMap<String, Instant>>,
    ssr: Arc<ssr::SsrManager>,
    /// Proxy client for peer Nodes and SSR upstreams: connect timeout only,
    /// so streamed responses are never cut off by a total timeout.
    proxy: reqwest::Client,
    pub ingress: Arc<certificates::IngressState>,
}

impl ServeState {
    pub fn new(
        client: ControlApiClient,
        node_id: Uuid,
        gateway_token: Option<String>,
        routes: Arc<routes::RouteTable>,
        config: &NodeConfig,
        ssr: Arc<ssr::SsrManager>,
    ) -> Self {
        Self {
            client,
            node_id,
            gateway_token,
            gateway_authentication: config.security.gateway_authentication,
            routes,
            cache_root: PathBuf::from(&config.serve.artifact_cache_root),
            targets: Mutex::new(HashMap::new()),
            preview_access_ttl: Duration::from_secs(
                config.serve.metadata_cache_ttl_seconds.clamp(1, 30),
            ),
            preview_grants: Mutex::new(HashMap::new()),
            ssr,
            ingress: Arc::new(certificates::IngressState::new(config, node_id)),
            proxy: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                // A site's Location belongs to its client. Following it here
                // could send gateway credentials outside the trusted peer.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("static reqwest options cannot fail"),
        }
    }
}

fn serve_router(state: Arc<ServeState>) -> axum::Router {
    axum::Router::new()
        .route("/_grass/health", get(health))
        .route(ROUTE_INVALIDATION_PATH, post(invalidate_routes))
        .route(PEER_PROXY_PREFIX, any(handle_peer_proxy))
        .route("/_grass/internal/proxy/", any(handle_peer_proxy))
        .route("/_grass/internal/proxy/{*path}", any(handle_peer_proxy))
        .fallback(route_public_request)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            ingress_request,
        ))
        .with_state(state)
}

async fn health(State(state): State<Arc<ServeState>>) -> Response {
    let ready = state.ingress.listeners_ready() && state.routes.revision().await.is_some();
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(serde_json::json!({ "ready": ready })),
    )
        .into_response()
}

/// Handles the two reserved public ingress endpoints before Host deployment
/// resolution. HTTPS still requires exact agreement between SNI and HTTP Host.
async fn ingress_request(
    State(state): State<Arc<ServeState>>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(authority) = request.uri().authority() {
        let Ok(authority_host) = grass_validator::normalize_host(authority.host()) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if request.headers().contains_key(header::HOST) {
            if host_from_headers(request.headers()).as_deref() != Some(authority_host.as_str()) {
                return StatusCode::MISDIRECTED_REQUEST.into_response();
            }
        } else {
            let Ok(value) = HeaderValue::from_str(authority.as_str()) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            request.headers_mut().insert(header::HOST, value);
        }
    }
    if let Some(connection) = request.extensions().get::<tls::TlsConnection>() {
        let sni = grass_validator::normalize_host(&connection.server_name).ok();
        if sni.is_none()
            || host_from_headers(request.headers()) != sni
            || !sni
                .as_deref()
                .is_some_and(|hostname| state.ingress.certificate_for(hostname).is_some())
        {
            return StatusCode::MISDIRECTED_REQUEST.into_response();
        }
    }
    if let Some(token) = request
        .uri()
        .path()
        .strip_prefix(certificates::CHALLENGE_PREFIX)
    {
        if !matches!(
            *request.method(),
            axum::http::Method::GET | axum::http::Method::HEAD
        ) {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        let Some(host) = host_from_headers(request.headers()) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let Some(authorization) = state.ingress.challenge(&host, token) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        return (
            [
                (header::CONTENT_TYPE, "text/plain"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            if request.method() == axum::http::Method::HEAD {
                String::new()
            } else {
                authorization
            },
        )
            .into_response();
    }
    next.run(request).await
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
    let authenticated = matches!(
        gateway_origin(
            &headers,
            state.gateway_token.as_deref().unwrap_or_default(),
            state.gateway_authentication,
        ),
        Ok(GatewayOrigin::Authenticated)
    );
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
    let tls_addr = config
        .serve
        .tls
        .enabled
        .then(|| std::net::SocketAddr::new(config.serve.host, config.serve.tls.port));
    tokio::spawn(async move {
        let app = serve_router(state.clone());
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                warn!(operation = "node.serve.bind", %error, %addr, "serve listener bind failed");
                return;
            }
        };
        let tls_listener = match tls_addr {
            Some(addr) => match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => Some(listener),
                Err(error) => {
                    warn!(operation = "node.serve.tls.bind", %error, %addr, "HTTPS listener bind failed");
                    return;
                }
            },
            None => None,
        };
        state.ingress.http_ready.store(true, Ordering::Release);
        state
            .ingress
            .tls_ready
            .store(tls_listener.is_some(), Ordering::Release);
        info!(operation = "node.serve.start", %addr, "public serve listener started");
        let http = axum::serve(
            listener,
            app.clone()
                .into_make_service_with_connect_info::<SocketAddr>(),
        );
        let result = if let Some(listener) = tls_listener {
            info!(operation = "node.serve.tls.start", addr = ?tls_addr, "HTTPS listener started");
            tokio::select! {
                result = std::future::IntoFuture::into_future(http) => result.map_err(anyhow::Error::from),
                result = tls::serve(listener, app, state.ingress.clone()) => result,
            }
        } else {
            http.await.map_err(anyhow::Error::from)
        };
        state.ingress.http_ready.store(false, Ordering::Release);
        state.ingress.tls_ready.store(false, Ordering::Release);
        if let Err(error) = result {
            warn!(operation = "node.serve.stopped", %error, "serve listener stopped");
        }
    })
}

#[cfg(test)]
mod release_smoke;

#[cfg(test)]
mod tests;
