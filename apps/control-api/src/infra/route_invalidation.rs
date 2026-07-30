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

pub async fn invalidate_deployment(
    db: &sea_orm::DatabaseConnection,
    secret_key: &str,
    deployment_id: Uuid,
) -> anyhow::Result<()> {
    let base_urls = node::Entity::find()
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
    if base_urls.is_empty() {
        return Ok(());
    }
    invalidate_at_urls(
        &reqwest::Client::new(),
        &base_urls,
        &nodes::gateway_token(secret_key),
        deployment_id,
    )
    .await
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

    use super::invalidate_at_urls;

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
}
