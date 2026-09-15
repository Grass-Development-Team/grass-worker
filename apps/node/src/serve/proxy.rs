//! Streaming peer and SSR forwarding with boundary-specific header filtering.

use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
};
use grass_node_protocol::GatewayAuthenticationMode;

use super::{
    normalize_public_path,
    routing::{GATEWAY_HOP_HEADER, GATEWAY_TOKEN_HEADER, GatewayOrigin, PEER_PROXY_PREFIX},
    tls,
};

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
pub(super) async fn forward_to_ssr(
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
            .header(
                "x-forwarded-proto",
                if parts.extensions.get::<tls::TlsConnection>().is_some() {
                    "https"
                } else {
                    "http"
                },
            )
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

pub(super) async fn forward_to_gateway(
    proxy: &reqwest::Client,
    target_base_url: &str,
    gateway_token: &str,
    gateway_authentication: GatewayAuthenticationMode,
    client_addr: SocketAddr,
    request: Request,
) -> anyhow::Result<Response> {
    if matches!(gateway_authentication, GatewayAuthenticationMode::Token)
        && gateway_token.is_empty()
    {
        anyhow::bail!("destination gateway requires an outbound credential");
    }
    let (parts, body) = request.into_parts();
    // URL parsers resolve literal and encoded dot segments. Validate before
    // adding credentials so a public path cannot escape the peer endpoint.
    if normalize_public_path(parts.uri.path()).is_none() {
        anyhow::bail!("invalid gateway request path");
    }
    let mut url = url::Url::parse(target_base_url)
        .map_err(|error| anyhow::anyhow!("invalid target Serve Node URL: {error}"))?;
    url.set_path(&format!("{PEER_PROXY_PREFIX}{}", parts.uri.path()));
    url.set_query(parts.uri.query());
    url.set_fragment(None);
    if !url.path().starts_with(&format!("{PEER_PROXY_PREFIX}/")) {
        anyhow::bail!("gateway request escaped the peer endpoint");
    }

    let mut builder = proxy.request(parts.method, url);
    for (name, value) in &parts.headers {
        if is_hop_by_hop(name) || is_gateway_internal(name) || is_forwarded_metadata(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder = builder.header(GATEWAY_HOP_HEADER, "1");
    if matches!(gateway_authentication, GatewayAuthenticationMode::Token) {
        builder = builder.header(GATEWAY_TOKEN_HEADER, gateway_token);
    }
    builder = builder
        .header("x-forwarded-for", client_addr.ip().to_string())
        .header(
            "x-forwarded-proto",
            if parts.extensions.get::<tls::TlsConnection>().is_some() {
                "https"
            } else {
                "http"
            },
        );
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
