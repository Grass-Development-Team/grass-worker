use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use grass_node_protocol::{RouteSnapshotResponse, ServeRoute};
use tokio::sync::RwLock;

const ROUTE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct RouteSnapshot {
    revision: Option<String>,
    routes: HashMap<String, ServeRoute>,
}

#[derive(Default)]
pub struct RouteTable {
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

    pub async fn revision(&self) -> Option<String> {
        self.snapshot.read().await.revision.clone()
    }
}

async fn refresh_routes(
    client: &crate::client::ControlApiClient,
    table: &RouteTable,
) -> anyhow::Result<bool> {
    table.apply(client.route_snapshot().await?).await
}

pub fn spawn(
    client: crate::client::ControlApiClient,
    table: Arc<RouteTable>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(ROUTE_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match refresh_routes(&client, &table).await {
                Ok(true) => {
                    let revision = table.revision().await;
                    tracing::info!(
                        operation = "node.serve.routes.updated",
                        ?revision,
                        "Serve route snapshot updated"
                    );
                }
                Ok(false) => {}
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
    use std::sync::Arc;

    use axum::{Router, routing::get};
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
}
