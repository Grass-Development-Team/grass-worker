pub(crate) mod by_credential_id;

use axum::{Json, extract::State, response::IntoResponse};
use grass_git_source::GitTransport;
use serde::Deserialize;
use serde_json::json;

use crate::infra::audit as audits;
use crate::{
    domain::source_credentials::{
        self, CreateCredentialParams, CreateSecret, SourceCredentialError,
    },
    infra::{
        audit::CreateAuditEventParams,
        database::entity::{AuditEventResult, source_credential},
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route(
            "/teams/{team_id}/source-credentials",
            axum::routing::get(list).post(create),
        )
        .merge(by_credential_id::router())
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

pub async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.source_credentials.list";
    role.require_admin(OP)?;
    let credentials =
        source_credentials::list_for_team(crate::infra::http::database(&state, OP)?, role.team_id)
            .await
            .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(ListResponse {
        credentials: credentials.iter().map(credential_view).collect::<Vec<_>>(),
    }))
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
    let transaction = audits::AuditTransaction::begin(crate::infra::http::database(&state, OP)?)
        .await
        .map_err(|source| AppError::Infrastructure {
            op: OP,
            source: source.into(),
        })?;
    let credential = source_credentials::create(
        &*transaction,
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
    audit(
        &transaction,
        &role,
        &credential,
        "source_credential.created",
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
    Ok(ok_response(CreateResponse {
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
struct ListResponse {
    credentials: Vec<SourceCredentialResponse>,
}

#[derive(serde::Serialize)]
struct CreateResponse {
    credential: SourceCredentialResponse,
}
