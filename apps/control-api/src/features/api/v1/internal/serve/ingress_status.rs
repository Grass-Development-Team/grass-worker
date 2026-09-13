use axum::{Extension, Json, extract::State, response::IntoResponse};
use grass_node_protocol::InstalledCertificate;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};

use crate::{
    infra::{
        error::{AppError, ok_response},
        http::middlewares::node_auth::AuthenticatedNode,
    },
    state::ControlApiState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReportIngressStatusRequest {
    pub certificates: Vec<InstalledCertificate>,
    pub challenge_revision: String,
    pub tls_ready: bool,
}

#[derive(serde::Serialize)]
struct IngressStatusResponse {
    ok: bool,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route("/serve/ingress-status", axum::routing::post(ingress_status))
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

/// POST /api/v1/internal/serve/ingress-status
pub async fn ingress_status(
    State(state): State<ControlApiState>,
    Extension(AuthenticatedNode(node)): Extension<AuthenticatedNode>,
    Json(body): Json<ReportIngressStatusRequest>,
) -> Result<impl IntoResponse, AppError> {
    use crate::infra::database::entity::node_ingress_status as status;
    use sea_orm::{Set, sea_query::OnConflict};
    const OP: &str = "internal.serve.ingress_status";
    ensure_serve_node(&node, OP)?;
    let revision_valid = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if body.certificates.len() > 4096
        || (!body.challenge_revision.is_empty() && !revision_valid(&body.challenge_revision))
        || body
            .certificates
            .iter()
            .any(|c| !revision_valid(&c.revision))
    {
        return Err(AppError::Validation {
            op: OP,
            message: "ingress status requires bounded SHA256 revisions".to_owned(),
        });
    }
    let db = crate::infra::http::database(&state, OP)?;
    status::Entity::insert(status::ActiveModel {
        node_id: Set(node.id),
        certificates: Set(
            serde_json::to_value(body.certificates).expect("certificate status serializes")
        ),
        challenge_revision: Set(body.challenge_revision),
        tls_ready: Set(body.tls_ready),
        checked_at: Set(time::OffsetDateTime::now_utc()),
    })
    .on_conflict(
        OnConflict::column(status::Column::NodeId)
            .update_columns([
                status::Column::Certificates,
                status::Column::ChallengeRevision,
                status::Column::TlsReady,
                status::Column::CheckedAt,
            ])
            .to_owned(),
    )
    .exec(db)
    .await
    .map_err(|source| AppError::Infrastructure {
        op: OP,
        source: source.into(),
    })?;
    Ok(ok_response(IngressStatusResponse { ok: true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ingress_status_http_route_accepts_authenticated_node_and_rejects_unbounded_revisions()
    {
        use crate::{
            infra::{
                config::ControlApiConfig, database::entity::node_ingress_status,
                http::middlewares::node_auth::AuthenticatedNode,
            },
            state::ControlApiState,
        };
        use axum::{
            Extension, Router,
            body::Body,
            http::{Request, StatusCode},
            routing::post,
        };
        use tower::ServiceExt;
        let node = crate::domain::ingress::tests::node_fixture();
        let state = ControlApiState::new(ControlApiConfig::default(), "unused-test-config");
        let revision = "a".repeat(64);
        let status = node_ingress_status::Model {
            node_id: node.id,
            certificates: serde_json::json!([]),
            challenge_revision: revision.clone(),
            tls_ready: true,
            checked_at: time::OffsetDateTime::now_utc(),
        };
        state
            .database
            .set(
                sea_orm::MockDatabase::new(sea_orm::DbBackend::Postgres)
                    .append_query_results([vec![status]])
                    .into_connection(),
            )
            .unwrap();
        let app = Router::new()
            .route("/ingress-status", post(super::ingress_status))
            .layer(Extension(AuthenticatedNode(node)))
            .with_state(state);
        let request = |revision: String| {
            Request::builder().method("POST").uri("/ingress-status").header("content-type","application/json").body(Body::from(serde_json::json!({"certificates":[],"challenge_revision":revision,"tls_ready":true}).to_string())).unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("invalid".to_owned()))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            app.oneshot(request(revision)).await.unwrap().status(),
            StatusCode::OK
        );
    }

    #[test]
    fn node_protocol_contract_is_compatible() {
        crate::test_support::assert_node_contract::<
            ReportIngressStatusRequest,
            grass_node_protocol::ReportIngressStatusRequest,
        >("ReportIngressStatusRequest");
    }
}
