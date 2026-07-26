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
        return AppError::Forbidden {
            op: OP,
            message: "node is disabled".to_owned(),
        }
        .into_response();
    }

    request.extensions_mut().insert(AuthenticatedNode(node));
    next.run(request).await
}
