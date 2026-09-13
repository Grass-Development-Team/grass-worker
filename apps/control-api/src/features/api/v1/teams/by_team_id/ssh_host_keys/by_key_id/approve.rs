use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::infra::audit as audits;
use crate::{
    domain::ssh_host_keys::{self, SshHostKeyError},
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, SshHostKeyStatus, ssh_host_key},
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/teams/{team_id}/ssh-host-keys/{key_id}/approve",
        axum::routing::post(approve),
    )
}

#[derive(Deserialize)]
pub struct HostKeyPath {
    pub key_id: Uuid,
}

fn view(key: &ssh_host_key::Model) -> ItemResponse {
    ItemResponse {
        id: key.id,
        host: key.host.clone(),
        port: key.port,
        key_type: key.key_type.clone(),
        fingerprint_sha256: key.fingerprint_sha256.clone(),
        status: match key.status {
            SshHostKeyStatus::Pending => "pending",
            SshHostKeyStatus::Approved => "approved",
            SshHostKeyStatus::Rejected => "rejected",
            SshHostKeyStatus::Superseded => "superseded",
        },
        approved_at: key.approved_at,
        last_seen_at: Some(key.last_seen_at),
    }
}

fn map_error(error: SshHostKeyError, op: &'static str) -> AppError {
    match error {
        SshHostKeyError::NotFound => AppError::NotFound {
            op,
            message: "SSH host key not found".to_owned(),
        },
        SshHostKeyError::Invalid => AppError::Validation {
            op,
            message: "SSH host key payload is invalid".to_owned(),
        },
        SshHostKeyError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
    }
}

async fn change_status(
    state: ControlApiState,
    role: TeamRole,
    path: HostKeyPath,
    status: SshHostKeyStatus,
    op: &'static str,
) -> Result<impl IntoResponse, AppError> {
    role.require_admin(op)?;
    let transaction = audits::AuditTransaction::begin(crate::infra::http::database(&state, op)?)
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    let key = ssh_host_keys::set_status(
        &*transaction,
        role.team_id,
        path.key_id,
        status,
        role.user_id,
    )
    .await
    .map_err(|error| map_error(error, op))?;
    let action = match key.status {
        SshHostKeyStatus::Approved => "ssh_host_key.approved",
        SshHostKeyStatus::Rejected => "ssh_host_key.rejected",
        _ => "ssh_host_key.updated",
    };
    audits::create_audit_event(
        &transaction,
        CreateAuditEventParams {
            actor_user_id: Some(role.user_id),
            actor_node_id: None,
            team_id: Some(role.team_id),
            action: action.to_owned(),
            target_type: "ssh_host_key".to_owned(),
            target_id: Some(key.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "host": key.host,
                "port": key.port,
                "key_type": key.key_type,
                "fingerprint_sha256": key.fingerprint_sha256,
            }),
        },
    )
    .await
    .map_err(|source| AppError::Infrastructure { op, source })?;
    transaction
        .commit()
        .await
        .map_err(|source| AppError::Infrastructure {
            op,
            source: source.into(),
        })?;
    Ok(ok_response(ChangeStatusResponse {
        host_key: view(&key),
    }))
}

pub async fn approve(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Path(path): Path<HostKeyPath>,
) -> Result<impl IntoResponse, AppError> {
    change_status(
        state,
        role,
        path,
        SshHostKeyStatus::Approved,
        "teams.ssh_host_keys.approve",
    )
    .await
}

#[derive(serde::Serialize)]
struct ItemResponse {
    id: uuid::Uuid,
    host: String,
    port: i32,
    key_type: String,
    fingerprint_sha256: String,
    status: &'static str,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    approved_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    last_seen_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct ChangeStatusResponse {
    host_key: ItemResponse,
}
