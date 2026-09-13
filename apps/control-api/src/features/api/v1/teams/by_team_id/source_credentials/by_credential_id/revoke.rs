use axum::{
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
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new().route(
        "/teams/{team_id}/source-credentials/{credential_id}/revoke",
        axum::routing::post(revoke),
    )
}

#[derive(Deserialize)]
struct CredentialPath {
    credential_id: Uuid,
}

fn credential_view(credential: &source_credential::Model) -> SourceCredentialResponse {
    SourceCredentialResponse {
        id: credential.id,
        team_id: credential.team_id,
        name: credential.name.clone(),
        kind: credential.kind.as_str(),
        host: credential.host.clone(),
        port: credential.port,
        username: credential.username.clone(),
        current_version_id: credential.current_version_id,
        revoked_at: credential.revoked_at,
        created_at: Some(credential.created_at),
        updated_at: Some(credential.updated_at),
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
    db: &impl audits::AuditConnection,
    role: &TeamRole,
    credential: &source_credential::Model,
    action: &str,
) -> anyhow::Result<()> {
    audits::create_audit_event(
        db,
        CreateAuditEventParams {
            actor_user_id: Some(role.user_id),
            actor_node_id: None,
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
    .await
}

async fn revoke(
    State(state): State<ControlApiState>,
    role: TeamRole,
    Path(path): Path<CredentialPath>,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.revoke";
    role.require_admin(OP)?;
    let transaction = audits::AuditTransaction::begin(crate::infra::http::database(&state, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let credential = source_credentials::revoke(&*transaction, role.team_id, path.credential_id)
        .await
        .map_err(|error| map_error(error, OP))?;
    audit(
        &transaction,
        &role,
        &credential,
        "source_credential.revoked",
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
    Ok(ok_response(RevokeResponse {
        credential: credential_view(&credential),
    }))
}

#[derive(serde::Serialize)]
struct SourceCredentialResponse {
    id: uuid::Uuid,
    team_id: uuid::Uuid,
    name: String,
    kind: &'static str,
    host: String,
    port: i32,
    username: Option<String>,
    current_version_id: Option<uuid::Uuid>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    revoked_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    created_at: Option<time::OffsetDateTime>,
    #[serde(serialize_with = "crate::infra::http::timestamps::serialize")]
    updated_at: Option<time::OffsetDateTime>,
}

#[derive(serde::Serialize)]
struct RevokeResponse {
    credential: SourceCredentialResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::database::entity::SourceCredentialKind;
    use crate::infra::database::entity::source_credential;
    use time::OffsetDateTime;
    use uuid::Uuid;
    #[test]
    fn credential_views_never_contain_secret_material() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let value = serde_json::to_value(credential_view(&source_credential::Model {
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
        }))
        .unwrap();
        let serialized = value.to_string();
        for forbidden in ["secret", "private_key", "passphrase", "encrypted_payload"] {
            assert!(!serialized.contains(forbidden));
        }
    }
}
