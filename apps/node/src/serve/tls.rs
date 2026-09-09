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
