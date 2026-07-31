use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use grass_node_protocol::{RouteSnapshotResponse, ServeRoute};
use tokio::sync::{Mutex, RwLock};

const ROUTE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RouteSnapshot {
    revision: Option<String>,
    routes: HashMap<String, ServeRoute>,
}

#[derive(Default)]
pub struct RouteTable {
    refresh: Mutex<()>,
    snapshot: RwLock<RouteSnapshot>,
}

impl RouteTable {
    pub async fn apply(&self, snapshot: RouteSnapshotResponse) -> anyhow::Result<bool> {
        if self.snapshot.read().await.revision.as_deref() == Some(snapshot.revision.as_str()) {
            return Ok(false);
        }

        let mut routes = HashMap::with_capacity(snapshot.routes.len());
        for mut route in snapshot.routes {
            let host = grass_validator::normalize_host(&route.host)
                .map_err(|error| anyhow::anyhow!("invalid route host {}: {error}", route.host))?;
            route.host.clone_from(&host);
            if routes.insert(host.clone(), route).is_some() {
                anyhow::bail!("route snapshot contains duplicate host {host}");
            }
        }

        let mut current = self.snapshot.write().await;
        *current = RouteSnapshot {
            revision: Some(snapshot.revision),
            routes,
        };
        Ok(true)
    }

    pub async fn lookup(&self, host: &str) -> Option<ServeRoute> {
        let host = grass_validator::normalize_host(host).ok()?;
        self.snapshot.read().await.routes.get(&host).cloned()
    }

    async fn clear(&self) -> bool {
        let mut snapshot = self.snapshot.write().await;
        let changed = snapshot.revision.is_some() || !snapshot.routes.is_empty();
        *snapshot = RouteSnapshot::default();
        changed
    }

    /// Removes every cached Host route for one deployment. Clearing the
    /// revision forces the next scheduled pull to re-apply the authoritative
    /// snapshot even if an invalidation races with snapshot generation.
    pub async fn remove_deployment(&self, deployment_id: uuid::Uuid) -> bool {
        let _refresh = self.refresh.lock().await;
        let mut snapshot = self.snapshot.write().await;
        let previous_len = snapshot.routes.len();
        snapshot
            .routes
            .retain(|_, route| route.deployment_id != deployment_id);
        let changed = snapshot.routes.len() != previous_len;
        if changed {
            snapshot.revision = None;
        }
        changed
    }

    pub async fn revision(&self) -> Option<String> {
        self.snapshot.read().await.revision.clone()
    }

    pub async fn local_deployment_ids(&self, node_id: uuid::Uuid) -> HashSet<uuid::Uuid> {
        self.snapshot
            .read()
            .await
            .routes
            .values()
            .filter(|route| route.target_node_id == node_id)
            .map(|route| route.deployment_id)
            .collect()
    }

    pub async fn deployment_ids(&self) -> HashSet<uuid::Uuid> {
        self.snapshot
            .read()
            .await
            .routes
            .values()
            .map(|route| route.deployment_id)
            .collect()
    }
}

async fn refresh_routes(
    client: &crate::client::ControlApiClient,
    table: &RouteTable,
) -> anyhow::Result<bool> {
    let _refresh = table.refresh.lock().await;
    match client.route_snapshot().await {
        Ok(snapshot) => table.apply(snapshot).await,
        Err(crate::client::RouteSnapshotError::AuthorizationRevoked) => Ok(table.clear().await),
        Err(error) => Err(error.into()),
    }
}

pub fn spawn(
    client: crate::client::ControlApiClient,
    table: Arc<RouteTable>,
    node_id: uuid::Uuid,
    ssr: Arc<super::ssr::SsrManager>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ROUTE_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match refresh_routes(&client, &table).await {
                Ok(changed) => {
                    let routed_here = table.local_deployment_ids(node_id).await;
                    let routed_anywhere = table.deployment_ids().await;
                    if let Err(error) = ssr.reconcile_routes(&routed_here, &routed_anywhere).await {
                        tracing::warn!(
                            operation = "node.serve.routes.reconcile_failed",
                            %error,
                            "failed to reconcile SSR services with Serve routes"
                        );
                    }
                    if changed {
                        let revision = table.revision().await;
                        tracing::info!(
                            operation = "node.serve.routes.updated",
                            ?revision,
                            "Serve route snapshot updated"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        operation = "node.serve.routes.failed",
                        %error,
                        "failed to refresh Serve routes"
                    );
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use axum::{Router, http::StatusCode, routing::get};
    use grass_node_protocol::{RouteSnapshotResponse, ServeResources, ServeRoute};
    use uuid::Uuid;

    use crate::client::ControlApiClient;

    use super::{RouteTable, refresh_routes};

    fn route(host: &str, deployment_id: Uuid) -> ServeRoute {
        ServeRoute {
            host: host.to_owned(),
            deployment_id,
            target_node_id: Uuid::now_v7(),
            target_base_url: "http://node-a:8080".to_owned(),
            resources: ServeResources {
                cpu_millicores: 50,
                memory_mb: 64,
                disk_mb: 256,
            },
            access: grass_node_protocol::ServeAccess::Public,
        }
    }

    #[tokio::test]
    async fn invalid_snapshot_preserves_last_valid_routes() {
        let table = RouteTable::default();
        let original_id = Uuid::now_v7();
        table
            .apply(RouteSnapshotResponse {
                revision: "revision-1".to_owned(),
                routes: vec![route("App.Example.com", original_id)],
            })
            .await
            .unwrap();
        assert_eq!(
            table.lookup("app.example.com").await.unwrap().deployment_id,
            original_id
        );

        let error = table
            .apply(RouteSnapshotResponse {
                revision: "revision-2".to_owned(),
                routes: vec![
                    route("app.example.com", Uuid::now_v7()),
                    route("APP.EXAMPLE.COM", Uuid::now_v7()),
                ],
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("duplicate host"));
        assert_eq!(table.revision().await.as_deref(), Some("revision-1"));
        assert_eq!(
            table.lookup("app.example.com").await.unwrap().deployment_id,
            original_id
        );
    }

    #[tokio::test]
    async fn refresh_applies_control_api_snapshot() {
        let deployment_id = Uuid::now_v7();
        let response_route = route("app.example.com", deployment_id);
        let app = Router::new().route(
            "/api/v1/internal/serve/routes",
            get(move || {
                let route = response_route.clone();
                async move {
                    axum::Json(serde_json::json!({
                        "code": 200,
                        "message": "OK",
                        "data": { "revision": "revision-1", "routes": [route] }
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();
        let table = Arc::new(RouteTable::default());

        refresh_routes(&client, &table).await.unwrap();

        assert_eq!(
            table.lookup("app.example.com").await.unwrap().deployment_id,
            deployment_id
        );
        server.abort();
    }

    #[tokio::test]
    async fn revoked_node_authorization_clears_cached_routes() {
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            let deployment_id = Uuid::now_v7();
            let table = Arc::new(RouteTable::default());
            table
                .apply(RouteSnapshotResponse {
                    revision: "before-revocation".to_owned(),
                    routes: vec![route("app.example.com", deployment_id)],
                })
                .await
                .unwrap();
            let app = Router::new().route(
                "/api/v1/internal/serve/routes",
                get(move || async move { status }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client =
                ControlApiClient::new(&format!("http://{address}"), "revoked-token").unwrap();

            let changed = refresh_routes(&client, &table).await.unwrap();

            assert!(changed);
            assert!(table.lookup("app.example.com").await.is_none());
            assert_eq!(table.revision().await, None);
            server.abort();
        }
    }

    #[tokio::test]
    async fn local_deployment_ids_follow_route_retargeting_without_host_duplicates() {
        let table = RouteTable::default();
        let local_node = Uuid::now_v7();
        let remote_node = Uuid::now_v7();
        let retained = Uuid::now_v7();
        let removed = Uuid::now_v7();
        let mut retained_preview = route("retained-preview.example.com", retained);
        retained_preview.target_node_id = local_node;
        let mut retained_production = route("retained.example.com", retained);
        retained_production.target_node_id = local_node;
        let mut removed_route = route("removed.example.com", removed);
        removed_route.target_node_id = local_node;
        table
            .apply(RouteSnapshotResponse {
                revision: "revision-1".to_owned(),
                routes: vec![retained_preview, retained_production.clone(), removed_route],
            })
            .await
            .unwrap();
        assert_eq!(
            table.local_deployment_ids(local_node).await,
            HashSet::from([retained, removed])
        );

        retained_production.target_node_id = remote_node;
        table
            .apply(RouteSnapshotResponse {
                revision: "revision-2".to_owned(),
                routes: vec![retained_production],
            })
            .await
            .unwrap();

        assert!(table.local_deployment_ids(local_node).await.is_empty());
    }

    #[tokio::test]
    async fn invalidated_deployment_cannot_be_readded_by_an_in_flight_stale_snapshot() {
        let table = Arc::new(RouteTable::default());
        let deployment_id = Uuid::now_v7();
        table
            .apply(RouteSnapshotResponse {
                revision: "before-withdrawal".to_owned(),
                routes: vec![route("app.example.com", deployment_id)],
            })
            .await
            .unwrap();

        let request_started = Arc::new(tokio::sync::Notify::new());
        let release_response = Arc::new(tokio::sync::Notify::new());
        let stale_route = route("app.example.com", deployment_id);
        let app = Router::new().route(
            "/api/v1/internal/serve/routes",
            get({
                let request_started = request_started.clone();
                let release_response = release_response.clone();
                move || {
                    let request_started = request_started.clone();
                    let release_response = release_response.clone();
                    let stale_route = stale_route.clone();
                    async move {
                        request_started.notify_one();
                        release_response.notified().await;
                        axum::Json(serde_json::json!({
                            "code": 200,
                            "message": "OK",
                            "data": {
                                "revision": "before-withdrawal",
                                "routes": [stale_route],
                            }
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = ControlApiClient::new(&format!("http://{address}"), "node-token").unwrap();
        let refresh = tokio::spawn({
            let table = table.clone();
            async move { refresh_routes(&client, &table).await.unwrap() }
        });
        request_started.notified().await;
        let invalidation = tokio::spawn({
            let table = table.clone();
            async move { table.remove_deployment(deployment_id).await }
        });
        tokio::task::yield_now().await;
        release_response.notify_one();

        refresh.await.unwrap();
        assert!(invalidation.await.unwrap());
        assert!(table.lookup("app.example.com").await.is_none());
        server.abort();
    }
}
