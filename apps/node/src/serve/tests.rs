use super::*;
use super::{
    paths::resolve_not_found_file,
    preview::*,
    proxy::{forward_to_gateway, forward_to_ssr},
    response::error_page,
    routing::{
        GATEWAY_HOP_HEADER, GATEWAY_TOKEN_HEADER, RouteAction, route_action,
        strip_peer_proxy_prefix,
    },
};
use axum::body::Body;
use grass_node_protocol::{ServeAccess, ServeRoute};

fn static_site(spa: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grass-serve-{}", uuid::Uuid::now_v7().simple()));
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
    let external = gateway_origin(&headers, token, GatewayAuthenticationMode::Token).unwrap();
    assert_eq!(external, GatewayOrigin::External);
    assert_eq!(
        route_action(Uuid::nil(), Uuid::now_v7(), external).unwrap(),
        RouteAction::Proxy
    );

    headers.insert("x-grass-gateway-token", token.parse().unwrap());
    headers.insert("x-grass-gateway-hop", "1".parse().unwrap());
    let authenticated = gateway_origin(&headers, token, GatewayAuthenticationMode::Token).unwrap();
    assert_eq!(authenticated, GatewayOrigin::Authenticated);
    assert!(route_action(Uuid::nil(), Uuid::now_v7(), authenticated).is_err());

    headers.insert("x-grass-gateway-token", "wrong-token".parse().unwrap());
    assert!(gateway_origin(&headers, token, GatewayAuthenticationMode::Token).is_err());
    headers.insert("x-grass-gateway-token", token.parse().unwrap());
    headers.insert("x-grass-gateway-hop", "2".parse().unwrap());
    assert!(gateway_origin(&headers, token, GatewayAuthenticationMode::Token).is_err());

    headers.remove(GATEWAY_TOKEN_HEADER);
    headers.insert(GATEWAY_HOP_HEADER, "1".parse().unwrap());
    assert_eq!(
        gateway_origin(&headers, token, GatewayAuthenticationMode::None).unwrap(),
        GatewayOrigin::Authenticated
    );
    headers.insert(GATEWAY_HOP_HEADER, "2".parse().unwrap());
    assert!(gateway_origin(&headers, token, GatewayAuthenticationMode::None).is_err());
    headers.insert(GATEWAY_TOKEN_HEADER, token.parse().unwrap());
    assert!(gateway_origin(&headers, token, GatewayAuthenticationMode::None).is_err());
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
        GatewayAuthenticationMode::Token,
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
                region: "default".to_owned(),
                deployment_id,
                target_node_id: Uuid::now_v7(),
                target_base_url: format!("http://{authority_address}"),
                gateway_authentication: Default::default(),
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
        Some("shared-gateway-token".to_owned()),
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
        .header(GATEWAY_HOP_HEADER, "1")
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
        Some("shared-gateway-token".to_owned()),
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

#[tokio::test]
async fn mixed_gateway_modes_deliver_bound_hosts_and_reject_second_hops() {
    use grass_node_protocol::{RouteSnapshotResponse, ServeResources};
    for source_mode in [
        GatewayAuthenticationMode::Token,
        GatewayAuthenticationMode::None,
    ] {
        for target_mode in [
            GatewayAuthenticationMode::Token,
            GatewayAuthenticationMode::None,
        ] {
            let directory = tempfile::tempdir().unwrap();
            tokio::fs::write(directory.path().join("index.html"), "regional site")
                .await
                .unwrap();
            let destination_id = Uuid::now_v7();
            let deployment_id = Uuid::now_v7();
            let destination_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let destination_url = format!("http://{}", destination_listener.local_addr().unwrap());
            let route = ServeRoute {
                host: "app.example.com".to_owned(),
                region: "eu-west".to_owned(),
                deployment_id,
                target_node_id: destination_id,
                target_base_url: destination_url.clone(),
                gateway_authentication: target_mode,
                resources: ServeResources {
                    cpu_millicores: 50,
                    memory_mb: 64,
                    disk_mb: 256,
                },
                access: ServeAccess::Public,
            };
            let make_state = |node_id, mode| {
                let mut config = NodeConfig::default();
                config.security.gateway_authentication = mode;
                Arc::new(ServeState::new(
                    ControlApiClient::new("http://127.0.0.1:9", "node-token").unwrap(),
                    node_id,
                    Some("shared-gateway-token".to_owned()),
                    Arc::new(routes::RouteTable::default()),
                    &config,
                    Arc::new(ssr::SsrManager::new(None, node_id, &config)),
                ))
            };
            let destination = make_state(destination_id, target_mode);
            destination
                .routes
                .apply(RouteSnapshotResponse {
                    revision: "target".to_owned(),
                    routes: vec![route.clone()],
                })
                .await
                .unwrap();
            destination.targets.lock().await.insert(
                deployment_id,
                ResolvedTarget::Static {
                    static_dir: directory.path().to_owned(),
                    spa_fallback: false,
                    not_found: None,
                },
            );
            let destination_server = tokio::spawn(async move {
                axum::serve(
                    destination_listener,
                    serve_router(destination).into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .unwrap();
            });
            let source = make_state(Uuid::now_v7(), source_mode);
            source
                .routes
                .apply(RouteSnapshotResponse {
                    revision: "source".to_owned(),
                    routes: vec![route],
                })
                .await
                .unwrap();
            let source_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let source_url = format!("http://{}", source_listener.local_addr().unwrap());
            let source_server = tokio::spawn(async move {
                axum::serve(
                    source_listener,
                    serve_router(source).into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .unwrap();
            });
            let client = reqwest::Client::new();
            let response = client
                .get(&source_url)
                .header(header::HOST, "app.example.com")
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{source_mode:?} -> {target_mode:?}"
            );
            assert_eq!(response.text().await.unwrap(), "regional site");
            let unbound = client
                .get(&source_url)
                .header(header::HOST, "unbound.example.com")
                .send()
                .await
                .unwrap();
            assert_eq!(unbound.status(), StatusCode::NOT_FOUND);
            let mut repeated = client
                .get(format!("{source_url}{PEER_PROXY_PREFIX}/"))
                .header(header::HOST, "app.example.com")
                .header(GATEWAY_HOP_HEADER, "1");
            if source_mode == GatewayAuthenticationMode::Token {
                repeated = repeated.header(GATEWAY_TOKEN_HEADER, "shared-gateway-token");
            }
            assert_eq!(
                repeated.send().await.unwrap().status(),
                StatusCode::BAD_GATEWAY
            );
            source_server.abort();
            destination_server.abort();
        }
    }
}

#[tokio::test]
async fn missing_outbound_token_fails_before_contacting_destination() {
    let request = Request::builder()
        .header(header::HOST, "app.example.com")
        .body(Body::empty())
        .unwrap();
    let error = forward_to_gateway(
        &reqwest::Client::new(),
        "http://127.0.0.1:9",
        "",
        GatewayAuthenticationMode::Token,
        "127.0.0.1:1234".parse().unwrap(),
        request,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "destination gateway requires an outbound credential"
    );
    let mut headers = HeaderMap::new();
    headers.insert(GATEWAY_TOKEN_HEADER, "".parse().unwrap());
    headers.insert(GATEWAY_HOP_HEADER, "1".parse().unwrap());
    assert!(gateway_origin(&headers, "", GatewayAuthenticationMode::Token).is_err());
}

#[tokio::test]
async fn gateway_redirects_and_traversal_never_send_credentials_to_other_endpoints() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let captured = Arc::new(AtomicUsize::new(0));
    let capture_router = axum::Router::new().fallback({
        let captured = captured.clone();
        move || {
            let captured = captured.clone();
            async move {
                captured.fetch_add(1, Ordering::SeqCst);
                StatusCode::OK
            }
        }
    });
    let capture_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let capture_url = format!("http://{}/capture", capture_listener.local_addr().unwrap());
    let capture_server = tokio::spawn(async move {
        axum::serve(capture_listener, capture_router).await.unwrap();
    });

    let peer_requests = Arc::new(AtomicUsize::new(0));
    let peer_router = axum::Router::new().fallback({
        let destination = capture_url.clone();
        let peer_requests = peer_requests.clone();
        move |request: Request| {
            let destination = destination.clone();
            let peer_requests = peer_requests.clone();
            async move {
                peer_requests.fetch_add(1, Ordering::SeqCst);
                assert_eq!(
                    request.headers()[GATEWAY_TOKEN_HEADER],
                    "shared-gateway-token"
                );
                assert_eq!(request.headers()[GATEWAY_HOP_HEADER], "1");
                assert_eq!(request.headers()[header::HOST], "app.example.com");
                assert!(request.uri().path().starts_with(PEER_PROXY_PREFIX));
                (StatusCode::FOUND, [(header::LOCATION, destination)])
            }
        }
    });
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_url = format!("http://{}", peer_listener.local_addr().unwrap());
    let peer_server = tokio::spawn(async move {
        axum::serve(peer_listener, peer_router).await.unwrap();
    });

    let mut config = NodeConfig::default();
    config.security.gateway_authentication = GatewayAuthenticationMode::None;
    let node_id = Uuid::now_v7();
    let routes = Arc::new(routes::RouteTable::default());
    routes
        .apply(grass_node_protocol::RouteSnapshotResponse {
            revision: "security".to_owned(),
            routes: vec![ServeRoute {
                host: "app.example.com".to_owned(),
                region: "default".to_owned(),
                deployment_id: Uuid::now_v7(),
                target_node_id: Uuid::now_v7(),
                target_base_url: peer_url.clone(),
                gateway_authentication: GatewayAuthenticationMode::Token,
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
    let state = Arc::new(ServeState::new(
        ControlApiClient::new("http://127.0.0.1:9", "node-token").unwrap(),
        node_id,
        Some("shared-gateway-token".to_owned()),
        routes,
        &config,
        Arc::new(ssr::SsrManager::new(None, node_id, &config)),
    ));
    let source_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_address = source_listener.local_addr().unwrap();
    let router = serve_router(state.clone());
    let source_server = tokio::spawn(async move {
        axum::serve(
            source_listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = browser
        .get(format!("http://{source_address}/redirect"))
        .header(header::HOST, "app.example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()[header::LOCATION], capture_url);
    assert_eq!(peer_requests.load(Ordering::SeqCst), 1);
    assert_eq!(captured.load(Ordering::SeqCst), 0);

    for path in [
        "/../../../_grass/internal/routes/invalidate",
        "/%2e%2e/%2e%2e/%2e%2e/_grass/internal/routes/invalidate",
        "/.%2E/.%2E/.%2E/_grass/internal/routes/invalidate",
        "/%5c../%5c../_grass/internal/routes/invalidate",
    ] {
        // Send raw HTTP to preserve the malicious path instead of the
        // test HTTP client's own URL parser normalizing it in advance.
        let mut stream = tokio::net::TcpStream::connect(source_address)
            .await
            .unwrap();
        stream.write_all(format!("POST {path} HTTP/1.1\r\nHost: app.example.com\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert!(
            response.starts_with(b"HTTP/1.1 400"),
            "{path}: {}",
            String::from_utf8_lossy(&response)
        );
        let request = Request::builder()
            .uri(path)
            .header(header::HOST, "app.example.com")
            .body(Body::empty())
            .unwrap();
        let error = forward_to_gateway(
            &state.proxy,
            &peer_url,
            "shared-gateway-token",
            GatewayAuthenticationMode::Token,
            source_address,
            request,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "invalid gateway request path");
    }
    assert_eq!(peer_requests.load(Ordering::SeqCst), 1);
    assert_eq!(captured.load(Ordering::SeqCst), 0);
    source_server.abort();
    peer_server.abort();
    capture_server.abort();
}
