use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::source_credentials::{self, SourceCredentialError},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, source_credential},
        error::{AppError, ok_response},
        http::extractors::Session,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/projects/{project_id}/source-credential",
        axum::routing::delete(unbind).get(get).post(bind),
    )
}

#[derive(Deserialize)]
struct BindCredentialRequest {
    credential_id: Uuid,
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
    db: &impl audits::AuditConnection,
    access: &crate::domain::project_access::ProjectAccess,
    actor_user_id: Uuid,
    credential: &source_credential::Model,
    action: &str,
) -> anyhow::Result<()> {
    audits::create_audit_event(
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
    .await
}

async fn get(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.get";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    let credential = source_credentials::bound_credential(
        crate::infra::http::database(&state, OP)?,
        access.project.id,
    )
    .await
    .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(GetResponse {
        credential: credential.map(|credential| GetCredentialResponse {
            id: credential.id,
            name: credential.name.clone(),
            kind: (credential.kind.as_str()).to_owned(),
            host: credential.host.clone(),
            port: credential.port,
            username: credential.username.clone(),
            revoked: credential.revoked_at.is_some(),
        }),
    }))
}

async fn bind(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
    Json(body): Json<BindCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.bind";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_admin(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
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
    .await
    .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(BindResponse {
        credential_id: credential.id,
    }))
}

async fn unbind(
    State(state): State<ControlApiState>,
    session: Session,
    Path(project_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "projects.source_credential.unbind";
    let access = crate::domain::project_access::load(
        &state,
        session.data.user_id,
        project_id,
        crate::domain::project_access::ProjectScope::Active,
        OP,
    )
    .await?;
    access.require_admin(OP)?;
    let db = crate::infra::http::database(&state, OP)?;
    let transaction = audits::AuditTransaction::begin(db)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let db = &transaction;
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
        .await
        .map_err(|source| AppError::Infrastructure { op: OP, source })?;
    }
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    Ok(ok_response(UnbindResponse { unbound: true }))
}

#[derive(serde::Serialize)]
struct GetCredentialResponse {
    id: uuid::Uuid,
    name: String,
    kind: String,
    host: String,
    port: i32,
    username: Option<String>,
    revoked: bool,
}

#[derive(serde::Serialize)]
struct GetResponse {
    credential: Option<GetCredentialResponse>,
}

#[derive(serde::Serialize)]
struct BindResponse {
    credential_id: uuid::Uuid,
}

#[derive(serde::Serialize)]
struct UnbindResponse {
    unbound: bool,
}
