use std::time::Duration;

use futures_util::future::join_all;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domain::nodes,
    infra::database::entity::{NodeStatus, node},
};

const INVALIDATION_PATH: &str = "/_grass/internal/routes/invalidate";

#[derive(Serialize)]
struct RouteInvalidationRequest {
    deployment_id: Uuid,
}

async fn invalidate_at_urls(
    client: &reqwest::Client,
    base_urls: &[String],
    gateway_token: &str,
    deployment_id: Uuid,
) -> anyhow::Result<()> {
    let requests = base_urls.iter().map(|base_url| async move {
        let mut endpoint = url::Url::parse(base_url)
            .map_err(|error| anyhow::anyhow!("invalid Serve Node base URL: {error}"))?;
        endpoint.set_path(INVALIDATION_PATH);
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let response = client
            .post(endpoint)
            .header("x-grass-gateway-token", gateway_token)
            .timeout(Duration::from_secs(3))
            .json(&RouteInvalidationRequest { deployment_id })
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("Serve Node rejected route invalidation");
        }
        Ok::<(), anyhow::Error>(())
    });
    let failures = join_all(requests)
        .await
        .into_iter()
        .filter_map(Result::err)
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} Serve Node(s) did not acknowledge route invalidation: {}",
            failures.len(),
            failures
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
}

async fn invalidate_best_effort_at_urls(
    client: &reqwest::Client,
    base_urls: &[String],
    gateway_token: &str,
    deployment_id: Uuid,
) {
    if let Err(error) = invalidate_at_urls(client, base_urls, gateway_token, deployment_id).await {
        tracing::warn!(
            operation = "routes.invalidate.inactive_nodes_failed",
            %deployment_id,
            %error,
            "failed to invalidate routes on one or more inactive Serve Nodes"
        );
    }
}

pub async fn invalidate_deployment(
    db: &sea_orm::DatabaseConnection,
    secret_key: &str,
    deployment_id: Uuid,
) -> anyhow::Result<()> {
    let active_base_urls = node::Entity::find()
        .select_only()
        .column(node::Column::BaseUrl)
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::Status.ne(NodeStatus::Disabled))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(node::Column::DeletedAt.is_null())
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let inactive_base_urls = node::Entity::find()
        .select_only()
        .column(node::Column::BaseUrl)
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(
            sea_orm::Condition::any()
                .add(node::Column::Status.eq(NodeStatus::Disabled))
                .add(node::Column::DeletedAt.is_not_null()),
        )
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let client = reqwest::Client::new();
    let gateway_token = nodes::gateway_token(secret_key);
    let (_, active_result) = tokio::join!(
        invalidate_best_effort_at_urls(&client, &inactive_base_urls, &gateway_token, deployment_id,),
        invalidate_at_urls(&client, &active_base_urls, &gateway_token, deployment_id,),
    );
    active_result
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use serde_json::Value;
    use uuid::Uuid;

    use super::{invalidate_at_urls, invalidate_best_effort_at_urls};

    #[tokio::test]
    async fn broadcasts_authenticated_route_invalidation_to_serve_nodes() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let deployment_id = Uuid::now_v7();
        let app = Router::new().route(
            "/_grass/internal/routes/invalidate",
            post({
                let received = received.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let received = received.clone();
                    async move {
                        received.lock().unwrap().push((headers, body));
                        StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        invalidate_at_urls(
            &reqwest::Client::new(),
            &[format!("http://{address}")],
            "derived-gateway-token",
            deployment_id,
        )
        .await
        .unwrap();

        let received = received.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].0["x-grass-gateway-token"],
            "derived-gateway-token"
        );
        assert_eq!(received[0].1["deployment_id"], deployment_id.to_string());
        server.abort();
    }

    #[tokio::test]
    async fn inactive_node_failures_do_not_block_route_invalidation() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let deployment_id = Uuid::now_v7();
        let app = Router::new().route(
            "/_grass/internal/routes/invalidate",
            post({
                let received = received.clone();
                move |Json(body): Json<Value>| {
                    let received = received.clone();
                    async move {
                        received.lock().unwrap().push(body);
                        StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let unavailable_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unavailable_address = unavailable_listener.local_addr().unwrap();
        drop(unavailable_listener);

        invalidate_best_effort_at_urls(
            &reqwest::Client::new(),
            &[
                format!("http://{address}"),
                format!("http://{unavailable_address}"),
            ],
            "derived-gateway-token",
            deployment_id,
        )
        .await;

        assert_eq!(received.lock().unwrap().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn active_node_failures_block_route_invalidation() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let error = invalidate_at_urls(
            &reqwest::Client::new(),
            &[format!("http://{address}")],
            "derived-gateway-token",
            Uuid::now_v7(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("did not acknowledge"));
    }
}
