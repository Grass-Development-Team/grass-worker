//! Bearer-token authentication for the internal Node API.
//!
//! Tokens are per Node; the database stores only the SHA-256 hash. Revoked
//! tokens are also cached so revocation applies immediately without waiting
//! for the row update to propagate.

use axum::{
    body::Body,
    extract::State,
    http::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use grass_cache::Cache;
use subtle::ConstantTimeEq;

use crate::{
    domain::nodes,
    infra::{database::entity::node, error::AppError},
    state::ControlApiState,
};

/// The authenticated Node attached to internal requests.
#[derive(Clone)]
pub struct AuthenticatedNode(pub node::Model);

pub fn revoked_token_key(token_hash: &str) -> String {
    format!("node:token:revoked:{token_hash}")
}

pub async fn node_auth_middleware(
    State(state): State<ControlApiState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    const OP: &str = "internal.node_auth";

    let token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty());

    let Some(token) = token else {
        return AppError::Unauthorized {
            op: OP,
            message: "node token required".to_owned(),
        }
        .into_response();
    };

    let token_hash = grass_token::hash_token(token);

    if let Some(cache) = state.try_cache() {
        match cache.get(&revoked_token_key(&token_hash)).await {
            Ok(Some(_)) => {
                return AppError::Unauthorized {
                    op: OP,
                    message: "node token has been revoked".to_owned(),
                }
                .into_response();
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(operation = OP, %error, "revocation cache read failed");
            }
        }
    }

    let Some(db) = state.try_database() else {
        return AppError::Internal {
            op: OP,
            message: "database not available".to_owned(),
        }
        .into_response();
    };

    let node = match nodes::find_by_token_hash(db, &token_hash).await {
        Ok(Some(node)) => node,
        Ok(None) => {
            return AppError::Unauthorized {
                op: OP,
                message: "invalid node token".to_owned(),
            }
            .into_response();
        }
        Err(source) => {
            return AppError::Infrastructure { op: OP, source }.into_response();
        }
    };

    // The lookup is by hash already; the constant-time comparison guards
    // against timing differences in the (theoretical) case of hash-prefix
    // collisions in the index lookup path.
    let matches: bool = node
        .token_hash
        .as_bytes()
        .ct_eq(token_hash.as_bytes())
        .into();
    if !matches {
        return AppError::Unauthorized {
            op: OP,
            message: "invalid node token".to_owned(),
        }
        .into_response();
    }

    if matches!(
        node.status,
        crate::infra::database::entity::NodeStatus::Disabled
    ) {
        let authenticated_node = AuthenticatedNode(node);
        let mut response = AppError::Forbidden {
            op: OP,
            message: "node is disabled".to_owned(),
        }
        .into_response();
        response.extensions_mut().insert(authenticated_node);
        return response;
    }

    let authenticated_node = AuthenticatedNode(node);
    request.extensions_mut().insert(authenticated_node.clone());
    let mut response = next.run(request).await;
    response.extensions_mut().insert(authenticated_node);
    response
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use sea_orm::MockDatabase;
    use time::OffsetDateTime;
    use tower::ServiceExt;

    use crate::{
        infra::{config::ControlApiConfig, database::entity::NodeStatus},
        state::ControlApiState,
    };

    use super::{AuthenticatedNode, node_auth_middleware};

    #[tokio::test]
    async fn disabled_authenticated_node_is_preserved_on_forbidden_response() {
        let token = "disabled-node-token";
        let node = crate::infra::database::entity::node::Model {
            id: uuid::Uuid::now_v7(),
            name: "disabled-node".to_owned(),
            token_hash: grass_token::hash_token(token),
            status: NodeStatus::Disabled,
            build_enabled: true,
            serve_enabled: false,
            build_concurrency: 1,
            base_url: None,
            work_root: None,
            capacity_cpu_millicores: 0,
            capacity_memory_mb: 0,
            capacity_disk_mb: 0,
            max_deployments: 0,
            metadata: serde_json::json!({}),
            last_heartbeat_at: None,
            desired_config: None,
            desired_config_revision: 0,
            effective_config: None,
            effective_config_revision: 0,
            config_sync_status: crate::infra::database::entity::NodeConfigSyncStatus::Pending,
            config_sync_error: None,
            node_token_configured: false,
            config_updated_at: None,
            config_applied_at: None,
            deleted_at: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };
        let db = MockDatabase::new(sea_orm::DbBackend::Postgres)
            .append_query_results([[node.clone()]])
            .into_connection();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused.toml");
        assert!(state.database.set(db).is_ok());
        let app = Router::new()
            .route("/internal-test", get(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                node_auth_middleware,
            ))
            .with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/internal-test")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            response
                .extensions()
                .get::<AuthenticatedNode>()
                .map(|authenticated| authenticated.0.id),
            Some(node.id)
        );
    }
}
