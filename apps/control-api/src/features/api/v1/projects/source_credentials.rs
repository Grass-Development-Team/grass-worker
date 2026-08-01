use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        source_credentials::{self, SourceCredentialError},
    },
    infra::{
        database::entity::{AuditEventResult, source_credential},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct BindCredentialRequest {
    pub credential_id: Uuid,
}

fn map_error(error: SourceCredentialError, op: &'static str) -> AppError {
    match error {
        SourceCredentialError::NotFound => AppError::NotFound {
            op,
            message: "source credential not found".to_owned(),
        },
        SourceCredentialError::Revoked => AppError::Conflict {
            op,
            message: "source credential has been revoked".to_owned(),
        },
        SourceCredentialError::EndpointMismatch => AppError::Validation {
            op,
            message: "source credential does not match repository scheme, host, and port"
                .to_owned(),
        },
        SourceCredentialError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        SourceCredentialError::Other(source) => AppError::Infrastructure { op, source },
        _ => AppError::Internal {
            op,
            message: "source credential operation failed".to_owned(),
        },
    }
}

async fn audit_binding(
    db: &sea_orm::DatabaseConnection,
    access: &super::ProjectAccess,
    actor_user_id: Uuid,
    credential: &source_credential::Model,
    action: &str,
) {
    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(actor_user_id),
            actor_node_id: None,
            team_id: Some(access.team.id),
            action: action.to_owned(),
            target_type: "source_credential".to_owned(),
            target_id: Some(credential.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({ "project_id": access.project.id }),
        },
    )
    .await;
}

pub async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.get";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    let credential =
        source_credentials::bound_credential(super::database(&state, OP)?, access.project.id)
            .await
            .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(json!({
        "credential": credential.map(|credential| json!({
            "id": credential.id,
            "name": credential.name,
            "kind": credential.kind.as_str(),
            "host": credential.host,
            "port": credential.port,
            "username": credential.username,
            "revoked": credential.revoked_at.is_some(),
        }))
    })))
}

pub async fn bind(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<BindCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.bind";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let credential = source_credentials::bind_project(
        db,
        &access.project,
        body.credential_id,
        session.data.user_id,
    )
    .await
    .map_err(|error| map_error(error, OP))?;
    audit_binding(
        db,
        &access,
        session.data.user_id,
        &credential,
        "source_credential.bound",
    )
    .await;
    Ok(ok_response(json!({ "credential_id": credential.id })))
}

pub async fn unbind(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.unbind";
    let access = super::project_access(&state, &session, project_id, false, OP).await?;
    access.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let credential = source_credentials::bound_credential(db, access.project.id)
        .await
        .map_err(|error| map_error(error, OP))?;
    source_credentials::unbind_project(db, access.project.id)
        .await
        .map_err(|error| map_error(error, OP))?;
    if let Some(credential) = credential {
        audit_binding(
            db,
            &access,
            session.data.user_id,
            &credential,
            "source_credential.unbound",
        )
        .await;
    }
    Ok(ok_response(json!({ "unbound": true })))
}
