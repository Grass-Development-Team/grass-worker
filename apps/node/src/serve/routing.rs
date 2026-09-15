//! Host binding, gateway authentication and single-hop route dispatch.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use grass_node_protocol::GatewayAuthenticationMode;
use subtle::ConstantTimeEq;
use tracing::warn;
use uuid::Uuid;

use super::{
    ServeState, local::serve_local, normalize_public_path, proxy::forward_to_gateway,
    response::error_page,
};

pub(super) const GATEWAY_TOKEN_HEADER: &str = "x-grass-gateway-token";
pub(super) const GATEWAY_HOP_HEADER: &str = "x-grass-gateway-hop";
pub(super) const PEER_PROXY_PREFIX: &str = "/_grass/internal/proxy";
pub(super) const ROUTE_INVALIDATION_PATH: &str = "/_grass/internal/routes/invalidate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GatewayOrigin {
    External,
    Authenticated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RouteAction {
    Local,
    Proxy,
}

pub(super) fn gateway_origin(
    headers: &HeaderMap,
    expected_token: &str,
    authentication: GatewayAuthenticationMode,
) -> Result<GatewayOrigin, &'static str> {
    let token = headers.get(GATEWAY_TOKEN_HEADER);
    let hop = headers.get(GATEWAY_HOP_HEADER);
    if matches!(authentication, GatewayAuthenticationMode::None) {
        return match (token, hop) {
            (None, None) => Ok(GatewayOrigin::External),
            (None, Some(hop)) if hop.to_str().ok() == Some("1") => Ok(GatewayOrigin::Authenticated),
            (None, Some(_)) => Err("invalid gateway hop"),
            (Some(_), _) => Err("gateway token is not accepted in none mode"),
        };
    }
    match (token, hop) {
        (None, None) => Ok(GatewayOrigin::External),
        (Some(token), Some(hop)) => {
            let token = token.to_str().map_err(|_| "invalid gateway token")?;
            let hop = hop.to_str().map_err(|_| "invalid gateway hop")?;
            let valid_token: bool = token.as_bytes().ct_eq(expected_token.as_bytes()).into();
            if expected_token.is_empty() || !valid_token {
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

pub(super) fn route_action(
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

pub(super) fn host_from_headers(headers: &HeaderMap) -> Option<String> {
    if headers.get_all(header::HOST).iter().count() != 1 {
        return None;
    }
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

pub(super) fn strip_peer_proxy_prefix(request: &mut Request) -> Result<(), &'static str> {
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

pub(super) async fn route_public_request(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    request: Request,
) -> Response {
    let origin = match gateway_origin(
        request.headers(),
        state.gateway_token.as_deref().unwrap_or_default(),
        state.gateway_authentication,
    ) {
        Ok(origin) => origin,
        Err(_) => {
            return error_page(
                StatusCode::FORBIDDEN,
                "This gateway request is not authorized.",
            );
        }
    };
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
    if normalize_public_path(request.uri().path()).is_none() {
        return error_page(
            StatusCode::BAD_REQUEST,
            "The requested path is not allowed.",
        );
    }
    match route_action(state.node_id, route.target_node_id, origin) {
        Ok(RouteAction::Proxy) => {
            return match forward_to_gateway(
                &state.proxy,
                &route.target_base_url,
                state.gateway_token.as_deref().unwrap_or_default(),
                route.gateway_authentication,
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
        Err(_) => {
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The gateway route snapshot points to another Serve Node.",
            );
        }
    }

    serve_local(state, route, client_addr, origin, request).await
}

pub(super) async fn handle_peer_proxy(
    State(state): State<Arc<ServeState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    mut request: Request,
) -> Response {
    if !matches!(
        gateway_origin(
            request.headers(),
            state.gateway_token.as_deref().unwrap_or_default(),
            state.gateway_authentication,
        ),
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
