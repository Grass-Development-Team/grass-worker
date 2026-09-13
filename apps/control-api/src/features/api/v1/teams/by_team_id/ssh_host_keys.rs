pub(crate) mod by_key_id;

use axum::{extract::State, response::IntoResponse};

use crate::{
    domain::ssh_host_keys::{self, SshHostKeyError},
    infra::{
        database::entity::{SshHostKeyStatus, ssh_host_key},
        error::{AppError, ok_response},
        http::extractors::TeamRole,
    },
    state::ControlApiState,
};

pub(crate) fn router() -> axum::Router<crate::state::ControlApiState> {
    axum::Router::new()
        .route("/teams/{team_id}/ssh-host-keys", axum::routing::get(list))
        .merge(by_key_id::router())
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

async fn list(
    State(state): State<ControlApiState>,
    role: TeamRole,
) -> Result<impl IntoResponse, AppError> {
    const OP: &str = "teams.ssh_host_keys.list";
    role.require_admin(OP)?;
    let keys =
        ssh_host_keys::list_for_team(crate::infra::http::database(&state, OP)?, role.team_id)
            .await
            .map_err(|error| map_error(error, OP))?;
    Ok(ok_response(ListResponse {
        host_keys: keys.iter().map(view).collect::<Vec<_>>(),
    }))
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
struct ListResponse {
    host_keys: Vec<ItemResponse>,
}
