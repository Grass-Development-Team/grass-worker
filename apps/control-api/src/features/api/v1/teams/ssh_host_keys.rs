use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        ssh_host_keys::{self, SshHostKeyError},
    },
    infra::{
        database::entity::{AuditEventResult, SshHostKeyStatus, ssh_host_key},
        error::{AppError, ok_response},
        http::{extractors::TeamRole, timestamps::ts},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct HostKeyPath {
    pub key_id: Uuid,
}

fn view(key: &ssh_host_key::Model) -> Value {
    json!({
        "id": key.id,
        "host": key.host,
        "port": key.port,
        "key_type": key.key_type,
        "fingerprint_sha256": key.fingerprint_sha256,
        "status": match key.status {
            SshHostKeyStatus::Pending => "pending",
            SshHostKeyStatus::Approved => "approved",
            SshHostKeyStatus::Rejected => "rejected",
            SshHostKeyStatus::Superseded => "superseded",
        },
        "approved_at": ts(key.approved_at),
        "last_seen_at": ts(Some(key.last_seen_at)),
    })
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

pub async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.ssh_host_keys.list";
    role.require_admin(OP)?;
    let keys = ssh_host_keys::list_for_team(super::database(&state, OP)?, role.team_id)
        .await
        .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(json!({
        "host_keys": keys.iter().map(view).collect::<Vec<_>>()
    })))
}

async fn change_status(
    state: ControlApiState,
    role: TeamRole,
    path: HostKeyPath,
    status: SshHostKeyStatus,
    op: &'static str,
) -> Result<impl IntoResponse, AppError> {
    role.require_admin(op)?;
    let key = ssh_host_keys::set_status(
        super::database(&state, op)?,
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
    let _ = audits::create_audit_event(
        super::database(&state, op)?,
        CreateAuditEventParams {
            actor_user_id: Some(role.user_id),
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
    .await;
    Ok(ok_response(json!({ "host_key": view(&key) })))
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

pub async fn reject(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Path(path): Path<HostKeyPath>,
) -> Result<impl IntoResponse, AppError> {
    change_status(
        state,
        role,
        path,
        SshHostKeyStatus::Rejected,
        "teams.ssh_host_keys.reject",
    )
    .await
}
