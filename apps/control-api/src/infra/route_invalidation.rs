use std::time::Duration;

use futures_util::future::join_all;
use grass_node_protocol::{GatewayAuthenticationMode, NodeConfiguration};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
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
    gateway_token: Option<&str>,
    gateway_authentication: GatewayAuthenticationMode,
    deployment_id: Uuid,
) -> anyhow::Result<()> {
    let requests = base_urls.iter().map(|base_url| async move {
        let mut endpoint = url::Url::parse(base_url)
            .map_err(|error| anyhow::anyhow!("invalid Serve Node base URL: {error}"))?;
        endpoint.set_path(INVALIDATION_PATH);
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        let mut request = client.post(endpoint).header("x-grass-gateway-hop", "1");
        if matches!(gateway_authentication, GatewayAuthenticationMode::Token) {
            if let Some(gateway_token) = gateway_token {
                request = request.header("x-grass-gateway-token", gateway_token);
            }
        }
        let response = request
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
    gateway_token: Option<&str>,
    gateway_authentication: GatewayAuthenticationMode,
    deployment_id: Uuid,
) {
    if let Err(error) = invalidate_at_urls(
        client,
        base_urls,
        gateway_token,
        gateway_authentication,
        deployment_id,
    )
    .await
    {
        tracing::warn!(
            operation = "routes.invalidate.inactive_nodes_failed",
            %deployment_id,
            %error,
            "failed to invalidate routes on one or more inactive Serve Nodes"
        );
    }
}

fn finish_post_commit_invalidation(
    result: anyhow::Result<()>,
    deployment_id: Uuid,
    operation: &'static str,
) {
    if let Err(error) = result {
        tracing::warn!(
            operation,
            %deployment_id,
            %error,
            "route invalidation failed after the deployment change was committed"
        );
    }
}

pub async fn invalidate_deployment(
    db: &sea_orm::DatabaseConnection,
    secret_key: &str,
    deployment_id: Uuid,
) -> anyhow::Result<()> {
    let active_nodes = node::Entity::find()
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::Status.ne(NodeStatus::Disabled))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(node::Column::DeletedAt.is_null())
        .all(db)
        .await?;
    let inactive_nodes = node::Entity::find()
        .filter(node::Column::ServeEnabled.eq(true))
        .filter(node::Column::BaseUrl.is_not_null())
        .filter(
            sea_orm::Condition::any()
                .add(node::Column::Status.eq(NodeStatus::Disabled))
                .add(node::Column::DeletedAt.is_not_null()),
        )
        .all(db)
        .await?;
    let client = reqwest::Client::new();
    let gateway_token = nodes::gateway_token(secret_key);
    let endpoint_groups = |nodes: Vec<node::Model>| {
        let mut token = Vec::new();
        let mut none = Vec::new();
        for node in nodes {
            let Some(base_url) = node.base_url else {
                continue;
            };
            let mode = node
                .effective_config
                .as_ref()
                .and_then(|value| serde_json::from_value::<NodeConfiguration>(value.clone()).ok())
                .map(|config| config.security.gateway_authentication)
                .unwrap_or_default();
            match mode {
                GatewayAuthenticationMode::Token => token.push(base_url),
                GatewayAuthenticationMode::None => none.push(base_url),
            }
        }
        (token, none)
    };
    let (active_token, active_none) = endpoint_groups(active_nodes);
    let (inactive_token, inactive_none) = endpoint_groups(inactive_nodes);
    let (_, active_result) = tokio::join!(
        async {
            invalidate_best_effort_at_urls(
                &client,
                &inactive_token,
                Some(&gateway_token),
                GatewayAuthenticationMode::Token,
                deployment_id,
            )
            .await;
            invalidate_best_effort_at_urls(
                &client,
                &inactive_none,
                None,
                GatewayAuthenticationMode::None,
                deployment_id,
            )
            .await;
        },
        async {
            let mut results = Vec::new();
            results.push(
                invalidate_at_urls(
                    &client,
                    &active_token,
                    Some(&gateway_token),
                    GatewayAuthenticationMode::Token,
                    deployment_id,
                )
                .await,
            );
            results.push(
                invalidate_at_urls(
                    &client,
                    &active_none,
                    None,
                    GatewayAuthenticationMode::None,
                    deployment_id,
                )
                .await,
            );
            results
                .into_iter()
                .find_map(Result::err)
                .map_or(Ok(()), Err)
        },
    );
    active_result
}

pub async fn invalidate_deployment_best_effort(
    db: &sea_orm::DatabaseConnection,
    secret_key: &str,
    deployment_id: Uuid,
    operation: &'static str,
) {
    finish_post_commit_invalidation(
        invalidate_deployment(db, secret_key, deployment_id).await,
        deployment_id,
        operation,
    );
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use grass_node_protocol::GatewayAuthenticationMode;
    use serde_json::Value;
    use uuid::Uuid;

    use super::{
        finish_post_commit_invalidation, invalidate_at_urls, invalidate_best_effort_at_urls,
    };

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
            Some("derived-gateway-token"),
            GatewayAuthenticationMode::Token,
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
        assert_eq!(received[0].0["x-grass-gateway-hop"], "1");
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
            Some("derived-gateway-token"),
            GatewayAuthenticationMode::Token,
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
            Some("derived-gateway-token"),
            GatewayAuthenticationMode::Token,
            Uuid::now_v7(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("did not acknowledge"));
    }

    #[test]
    fn post_commit_invalidation_does_not_fail_a_completed_withdrawal() {
        finish_post_commit_invalidation(
            Err(anyhow::anyhow!("Serve Node unavailable")),
            Uuid::now_v7(),
            "deployments.unpublish",
        );
    }
}
