use axum::{
    Extension,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::ssr_leases,
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

pub(crate) mod by_lease_id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SsrLeaseResponse {
    pub lease_id: Uuid,
    pub expires_at_unix: i64,
    pub hour_block_start_unix: i64,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new()
        .route(
            "/serve/deployments/{deployment_id}/ssr-lease",
            axum::routing::post(acquire_ssr_lease),
        )
        .merge(by_lease_id::router())
}

fn ensure_serve_node(
    node: &crate::infra::database::entity::node::Model,
    op: &'static str,
) -> Result<(), AppError> {
    if !node.serve_enabled {
        return Err(AppError::Forbidden {
            op,
            message: "node does not have Serve capability".to_owned(),
        });
    }
    Ok(())
}

fn map_lease_error(error: ssr_leases::LeaseError, op: &'static str) -> AppError {
    match error {
        ssr_leases::LeaseError::ProcessQuota => AppError::QuotaExceeded {
            op,
            message: "quota exceeded: ssr_processes limit reached".to_owned(),
        },
        ssr_leases::LeaseError::HourQuota => AppError::QuotaExceeded {
            op,
            message: "quota exceeded: ssr_hours.monthly limit reached".to_owned(),
        },
        ssr_leases::LeaseError::NotAssigned => AppError::Forbidden {
            op,
            message: "deployment is not an SSR Serve assignment for this node".to_owned(),
        },
        ssr_leases::LeaseError::WrongNode => AppError::Forbidden {
            op,
            message: "SSR lease belongs to another node".to_owned(),
        },
        ssr_leases::LeaseError::NotFound => AppError::NotFound {
            op,
            message: "SSR lease not found or expired".to_owned(),
        },
        ssr_leases::LeaseError::Database(source) => AppError::Infrastructure { op, source },
    }
}

fn lease_response(
    lease: &crate::infra::database::entity::ssr_process_lease::Model,
) -> SsrLeaseResponse {
    SsrLeaseResponse {
        lease_id: lease.id,
        expires_at_unix: lease.expires_at.unix_timestamp(),
        hour_block_start_unix: lease.hour_block_start.unix_timestamp(),
    }
}

/// POST /api/v1/internal/serve/deployments/{deployment_id}/ssr-lease
pub async fn acquire_ssr_lease(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path(deployment_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.ssr_lease.acquire";
    ensure_serve_node(&node, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let lease = ssr_leases::acquire(db, deployment_id, node.id, time::OffsetDateTime::now_utc())
        .await
        .map_err(|error| map_lease_error(error, OP))?;
    Ok(ok_response(lease_response(&lease)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            SsrLeaseResponse,
            grass_node_protocol::SsrLeaseResponse,
        >("SsrLeaseResponse");
    }
}
