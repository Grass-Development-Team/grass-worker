use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{
    domain::deployments,
    infra::{
        database::entity::deployment,
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(serde::Serialize)]
struct EventsResponse {
    events: Vec<EventResponse>,
}

#[derive(serde::Serialize)]
struct EventResponse {
    id: Uuid,
    kind: &'static str,
    message: String,
    metadata: serde_json::Value,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: time::OffsetDateTime,
}

pub(crate) fn router() -> axum::Router<ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/deployments/{deployment_id}/events",
        axum::routing::get(events),
    )
}

async fn load_deployment(
    db: &sea_orm::DatabaseConnection,
    access: &crate::domain::project_access::ProjectAccess,
    deployment_id: Uuid,
    op: &'static str,
) -> Result<deployment::Model, AppError> {
    let deployment = deployments::get_by_id(db, deployment_id)
        .await
        .map_err(|source| AppError::Infrastructure { op, source })?
        .ok_or_else(|| AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        })?;
    if deployment.project_id != access.project.id {
        return Err(AppError::NotFound {
            op,
            message: "deployment not found".to_owned(),
        });
    }
    Ok(deployment)
}

fn event_kind_value(kind: &crate::infra::database::entity::DeploymentEventKind) -> &'static str {
    use crate::infra::database::entity::DeploymentEventKind as K;
    match kind {
        K::System => "system",
        K::Build => "build",
        K::Serve => "serve",
        K::Release => "release",
        K::Review => "review",
        K::Host => "host",
    }
}

/// GET /api/v1/projects/{project_id}/deployments/{deployment_id}/events
async fn events(
    State(state): State<ControlApiState>,
    session: Session,
    Path((project_id, deployment_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "deployments.events";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let db = crate::infra::http::database(&state, OP)?;
    let deployment = load_deployment(db, &access, deployment_id, OP).await?;

    let events = deployments::list_events(db, deployment.id)
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    Ok(ok_response(EventsResponse {
        events: events
            .iter()
            .map(|event| EventResponse {
                id: event.id,
                kind: event_kind_value(&event.kind),
                message: event.message.clone(),
                metadata: event.metadata.clone(),
                created_at: event.created_at,
            })
            .collect::<Vec<_>>(),
    }))
}
