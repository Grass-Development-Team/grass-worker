use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use grass_git_source::GitTransport;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domain::{
        audits::{self, CreateAuditEventParams},
        source_credentials::{self, CreateCredentialParams, CreateSecret, SourceCredentialError},
    },
    infra::{
        database::entity::{AuditEventResult, SourceCredentialKind, source_credential},
        error::{AppError, ok_response},
        http::{extractors::TeamRole, timestamps::ts},
    },
    state::ControlApiState,
};

#[derive(Deserialize)]
pub struct CredentialPath {
    pub credential_id: Uuid,
}

#[derive(Deserialize)]
pub struct CreateCredentialRequest {
    pub name: String,
    pub repository_url: String,
    pub username: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

#[derive(Deserialize)]
pub struct RotateCredentialRequest {
    pub username: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub private_key: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

fn credential_view(credential: &source_credential::Model) -> Value {
    json!({
        "id": credential.id,
        "team_id": credential.team_id,
        "name": credential.name,
        "kind": credential.kind.as_str(),
        "host": credential.host,
        "port": credential.port,
        "username": credential.username,
        "current_version_id": credential.current_version_id,
        "revoked_at": ts(credential.revoked_at),
        "created_at": ts(Some(credential.created_at)),
        "updated_at": ts(Some(credential.updated_at)),
    })
}

fn secret_for_transport(
    transport: GitTransport,
    username: String,
    secret: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
    op: &'static str,
) -> Result<CreateSecret, AppError> {
    match transport {
        GitTransport::Https => Ok(CreateSecret::Https {
            username,
            secret: secret.filter(|value| !value.is_empty()).ok_or_else(|| {
                AppError::Validation {
                    op,
                    message: "secret is required for an HTTPS credential".to_owned(),
                }
            })?,
        }),
        GitTransport::Ssh => Ok(CreateSecret::Ssh {
            username,
            private_key: private_key
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| AppError::Validation {
                    op,
                    message: "private_key is required for an SSH credential".to_owned(),
                })?,
            passphrase: passphrase.filter(|value| !value.is_empty()),
        }),
        GitTransport::Http | GitTransport::Git => Err(AppError::Validation {
            op,
            message: "credentials are only supported for HTTPS and SSH repositories".to_owned(),
        }),
    }
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
        SourceCredentialError::InvalidPayload => AppError::Validation {
            op,
            message: "source credential payload is invalid".to_owned(),
        },
        SourceCredentialError::EncryptionUnavailable => AppError::Internal {
            op,
            message: "source credential encryption is not configured".to_owned(),
        },
        SourceCredentialError::InvalidLease => AppError::Unauthorized {
            op,
            message: "source credential lease is invalid or expired".to_owned(),
        },
        SourceCredentialError::Database(source) => AppError::Infrastructure {
            op,
            source: source.into(),
        },
        SourceCredentialError::Other(source) => AppError::Infrastructure { op, source },
    }
}

async fn audit(
    state: &ControlApiState,
    role: &TeamRole,
    credential: &source_credential::Model,
    action: &str,
) {
    let Some(db) = state.try_database() else {
        return;
    };
    let _ = audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(role.user_id),
            team_id: Some(role.team_id),
            action: action.to_owned(),
            target_type: "source_credential".to_owned(),
            target_id: Some(credential.id),
            result: AuditEventResult::Success,
            reason: None,
            metadata: json!({
                "kind": credential.kind.as_str(),
                "host": credential.host,
                "port": credential.port,
            }),
        },
    )
    .await;
}

pub async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.list";
    role.require_admin(OP)?;
    let credentials = source_credentials::list_for_team(super::database(&state, OP)?, role.team_id)
        .await
        .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(json!({
        "credentials": credentials.iter().map(credential_view).collect::<Vec<_>>()
    })))
}

pub async fn create(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Json(body): Json<CreateCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.create";
    role.require_admin(OP)?;
    if body.name.trim().is_empty() || body.username.trim().is_empty() {
        return Err(AppError::Validation {
            op: OP,
            message: "name and username are required".to_owned(),
        });
    }
    let endpoint =
        grass_git_source::parse_repository_url(body.repository_url.trim()).map_err(|_| {
            AppError::Validation {
                op: OP,
                message: "repository_url is invalid or unsupported".to_owned(),
            }
        })?;
    let secret = secret_for_transport(
        endpoint.transport,
        body.username.trim().to_owned(),
        body.secret,
        body.private_key,
        body.passphrase,
        OP,
    )?;
    let keyring = state.config.read().unwrap().secrets.git_credentials.clone();
    let credential = source_credentials::create(
        super::database(&state, OP)?,
        &keyring,
        CreateCredentialParams {
            team_id: role.team_id,
            name: body.name.trim().to_owned(),
            endpoint,
            secret,
            actor_user_id: role.user_id,
        },
    )
    .await
    .map_err(|error| map_error(error, OP))?;
    audit(&state, &role, &credential, "source_credential.created").await;
    Ok(ok_response(
        json!({ "credential": credential_view(&credential) }),
    ))
}

pub async fn rotate(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Path(path): Path<CredentialPath>,
    Json(body): Json<RotateCredentialRequest>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.rotate";
    role.require_admin(OP)?;
    let db = super::database(&state, OP)?;
    let existing = source_credentials::get_for_team(db, role.team_id, path.credential_id)
        .await
        .map_err(|error| map_error(error, OP))?;
    let transport = match existing.kind {
        SourceCredentialKind::Https => GitTransport::Https,
        SourceCredentialKind::Ssh => GitTransport::Ssh,
    };
    let secret = secret_for_transport(
        transport,
        body.username.trim().to_owned(),
        body.secret,
        body.private_key,
        body.passphrase,
        OP,
    )?;
    let keyring = state.config.read().unwrap().secrets.git_credentials.clone();
    let credential = source_credentials::rotate(
        db,
        &keyring,
        role.team_id,
        path.credential_id,
        secret,
        role.user_id,
    )
    .await
    .map_err(|error| map_error(error, OP))?;
    audit(&state, &role, &credential, "source_credential.rotated").await;
    Ok(ok_response(
        json!({ "credential": credential_view(&credential) }),
    ))
}

pub async fn revoke(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Path(path): Path<CredentialPath>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.revoke";
    role.require_admin(OP)?;
    let credential = source_credentials::revoke(
        super::database(&state, OP)?,
        role.team_id,
        path.credential_id,
    )
    .await
    .map_err(|error| map_error(error, OP))?;
    audit(&state, &role, &credential, "source_credential.revoked").await;
    Ok(ok_response(
        json!({ "credential": credential_view(&credential) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn credential_views_never_contain_secret_material() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let value = credential_view(&source_credential::Model {
            id: Uuid::nil(),
            team_id: Uuid::nil(),
            name: "deploy".to_owned(),
            kind: SourceCredentialKind::Https,
            host: "example.com".to_owned(),
            port: 443,
            username: Some("git".to_owned()),
            current_version_id: Some(Uuid::nil()),
            revoked_at: None,
            created_by_user_id: None,
            created_at: now,
            updated_at: now,
        });
        let serialized = value.to_string();
        for forbidden in ["secret", "private_key", "passphrase", "encrypted_payload"] {
            assert!(!serialized.contains(forbidden));
        }
    }
}
