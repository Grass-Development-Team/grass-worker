use axum::{
    Extension,
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::ssr_leases,
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct ReleaseLeaseResponse {
    released: bool,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/serve/deployments/{deployment_id}/ssr-lease/{lease_id}/release",
        axum::routing::post(release_ssr_lease),
    )
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

/// POST /api/v1/internal/serve/deployments/{deployment_id}/ssr-lease/{lease_id}/release
pub async fn release_ssr_lease(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Path((deployment_id, lease_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "internal.serve.ssr_lease.release";
    ensure_serve_node(&node, OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let released = ssr_leases::release(
        db,
        lease_id,
        deployment_id,
        node.id,
        time::OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|error| map_lease_error(error, OP))?;
    Ok(ok_response(ReleaseLeaseResponse { released }))
}
