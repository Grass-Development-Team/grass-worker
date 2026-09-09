//! Native HTTPS listener with dynamic SNI certificates and connection metadata.

use std::{sync::Arc, time::Duration};

use axum::{Extension, extract::ConnectInfo};
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinSet};
use tokio_rustls::TlsAcceptor;

use super::certificates::IngressState;

/// This extension is created from the TLS connection, never from HTTP headers.
#[derive(Clone)]
pub struct TlsConnection {
    pub server_name: String,
}

pub fn configuration(ingress: Arc<IngressState>) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_cert_resolver(ingress);
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    // Every new connection must resolve the current certificate, including
    // after a host has been revoked or its previous certificate has expired.
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.send_tls13_tickets = 0;
    Ok(Arc::new(config))
}

pub async fn serve(
    listener: TcpListener,
    app: axum::Router,
    ingress: Arc<IngressState>,
) -> anyhow::Result<()> {
    let acceptor = TlsAcceptor::from(configuration(ingress)?);
    let permits = Arc::new(Semaphore::new(4096));
    // Dropping the parent listener task also aborts all of its connections.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            connection = listener.accept() => {
                let (stream, address) = connection?;
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                let acceptor = acceptor.clone();
                let app = app.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await else { return; };
                    let Some(server_name) = stream.get_ref().1.server_name().map(str::to_owned) else { return; };
                    let service = TowerToHyperService::new(app
                        .layer(Extension(ConnectInfo(address)))
                        .layer(Extension(TlsConnection { server_name })));
                    let mut builder = Builder::new(TokioExecutor::new());
                    builder.http1().timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(15));
                    if let Err(error) = builder.serve_connection_with_upgrades(TokioIo::new(stream), service).await {
                        tracing::debug!(operation = "node.serve.tls.connection", %error, "HTTPS connection closed");
                    }
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use grass_node_protocol::{
        CertificateBundle, CertificateBundlesResponse, RouteSnapshotResponse, ServeAccess,
        ServeResources, ServeRoute,
    };
    use uuid::Uuid;

    use super::*;
    use crate::{
        client::ControlApiClient,
        config::NodeConfig,
        serve::{ResolvedTarget, ServeState, routes::RouteTable, serve_router, ssr::SsrManager},
    };

    struct ServerTask(tokio::task::JoinHandle<()>);

    impl Drop for ServerTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn node(
        root: &Path,
        node_id: Uuid,
        region: &str,
        route: ServeRoute,
        tls_enabled: bool,
    ) -> Arc<ServeState> {
        let mut config = NodeConfig::default();
        config.node.region = region.to_owned();
        config.serve.tls.enabled = tls_enabled;
        config.serve.artifact_cache_root = root.join(node_id.to_string()).display().to_string();
        let client = ControlApiClient::new("http://127.0.0.1:1", "test-node-token").unwrap();
        let routes = Arc::new(RouteTable::default());
        routes
            .apply(RouteSnapshotResponse {
                revision: "full-regional-snapshot".to_owned(),
                routes: vec![route],
            })
            .await
            .unwrap();
        let manager = Arc::new(SsrManager::with_client(
            None,
            node_id,
            &config,
            client.clone(),
        ));
        let state = Arc::new(ServeState::new(
            client,
            node_id,
            "shared-test-gateway-token".to_owned(),
            routes,
            &config,
            manager,
        ));
        state.ingress.http_ready.store(true, Ordering::Release);
        state
            .ingress
            .tls_ready
            .store(tls_enabled, Ordering::Release);
        state
    }

    async fn entry(
        state: Arc<ServeState>,
        certificate: CertificateBundle,
    ) -> (SocketAddr, ServerTask) {
        state
            .ingress
            .apply(CertificateBundlesResponse {
                bundles: vec![certificate],
                ..Default::default()
            })
            .await
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            serve(listener, serve_router(state.clone()), state.ingress.clone())
                .await
                .unwrap();
        });
        (address, ServerTask(task))
    }

    /// A loopback TCP frontend exercises passthrough and connection failover,
    /// without depending on an external load-balancer process or a real CA.
    async fn frontend(
        backends: [SocketAddr; 2],
        selected: Arc<AtomicUsize>,
    ) -> (SocketAddr, ServerTask) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept() => {
                        let (mut downstream, _) = connection.unwrap();
                        let selected = selected.clone();
                        connections.spawn(async move {
                            for (index, backend) in backends.into_iter().enumerate() {
                                if let Ok(Ok(mut upstream)) = tokio::time::timeout(Duration::from_millis(500), tokio::net::TcpStream::connect(backend)).await {
                                    selected.store(index, Ordering::Release);
                                    let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
                                    break;
                                }
                            }
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        (address, ServerTask(task))
    }

    #[tokio::test]
    async fn surviving_tls_entry_keeps_serving_a_deployment_in_another_region() {
        const HOST: &str = "site.example.test";
        let root = tempfile::tempdir().unwrap();
        let deployment_id = Uuid::now_v7();
        let target_id = Uuid::now_v7();
        let target_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = ServeRoute {
            host: HOST.to_owned(),
            region: "us".to_owned(),
            deployment_id,
            target_node_id: target_id,
            target_base_url: format!("http://{}", target_listener.local_addr().unwrap()),
            resources: ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 128,
            },
            access: ServeAccess::Public,
        };
        let target = node(root.path(), target_id, "us", route.clone(), false).await;
        let artifact = root.path().join("site-artifact");
        tokio::fs::create_dir(&artifact).await.unwrap();
        tokio::fs::write(
            artifact.join("index.html"),
            "deployment stays on its original US Serve Node",
        )
        .await
        .unwrap();
        target.targets.lock().await.insert(
            deployment_id,
            ResolvedTarget::Static {
                static_dir: artifact,
                spa_fallback: false,
                not_found: None,
            },
        );
        let _target_task = ServerTask(tokio::spawn(async move {
            axum::serve(
                target_listener,
                serve_router(target).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        }));
        let generated = rcgen::generate_simple_self_signed(vec![HOST.to_owned()]).unwrap();
        let certificate = CertificateBundle {
            ingress_id: Uuid::now_v7(),
            hostname: HOST.to_owned(),
            certificate_pem: generated.cert.pem(),
            private_key_pem: generated.signing_key.serialize_pem(),
            issued_at_unix: None,
            expires_at_unix: None,
            revision: String::new(),
        };
        let a = node(root.path(), Uuid::now_v7(), "eu", route.clone(), true).await;
        let b = node(root.path(), Uuid::now_v7(), "eu", route, true).await;
        let (a_address, a_task) = entry(a, certificate.clone()).await;
        let (b_address, _b_task) = entry(b, certificate.clone()).await;
        let selected = Arc::new(AtomicUsize::new(usize::MAX));
        let (frontend_address, _frontend_task) =
            frontend([a_address, b_address], selected.clone()).await;
        let client = reqwest::Client::builder()
            .no_proxy()
            .add_root_certificate(
                reqwest::Certificate::from_pem(certificate.certificate_pem.as_bytes()).unwrap(),
            )
            .resolve(HOST, frontend_address)
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let url = format!("https://{HOST}:{}/", frontend_address.port());
        let initial = client.get(&url).send().await.unwrap();
        assert_eq!(initial.status(), axum::http::StatusCode::OK);
        assert_eq!(
            initial.text().await.unwrap(),
            "deployment stays on its original US Serve Node"
        );
        assert_eq!(selected.load(Ordering::Acquire), 0);

        a_task.0.abort();
        // Await termination so the first TLS listener and its active connection
        // tasks have been dropped before the next client connection is opened.
        let mut a_task = a_task;
        let _ = (&mut a_task.0).await;
        let after_failure = client.get(&url).send().await.unwrap();
        assert_eq!(after_failure.status(), axum::http::StatusCode::OK);
        assert_eq!(
            after_failure.text().await.unwrap(),
            "deployment stays on its original US Serve Node"
        );
        assert_eq!(selected.load(Ordering::Acquire), 1);
    }
}
