use grass_git_source::{GitTransport, RepositoryEndpoint};
use grass_node_protocol::GitCredential;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::infra::{
    config::secrets::GitCredentialEncryptionConfig,
    database::entity::{
        SourceCredentialKind, project, project_source_credential, source_credential,
        source_credential_lease, source_credential_version,
    },
};

const LEASE_TTL: Duration = Duration::minutes(5);

#[derive(Debug, thiserror::Error)]
pub enum SourceCredentialError {
    #[error("source credential was not found")]
    NotFound,
    #[error("source credential has been revoked")]
    Revoked,
    #[error("source credential does not match the repository endpoint")]
    EndpointMismatch,
    #[error("source credential payload is invalid")]
    InvalidPayload,
    #[error("source credential encryption is not configured")]
    EncryptionUnavailable,
    #[error("source credential lease is invalid or expired")]
    InvalidLease,
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredCredentialPayload {
    Https {
        username: String,
        secret: String,
    },
    Ssh {
        username: String,
        private_key: String,
        passphrase: Option<String>,
    },
}

impl StoredCredentialPayload {
    fn validate(&self) -> Result<(), SourceCredentialError> {
        match self {
            Self::Https { username, secret } => {
                if username.trim().is_empty()
                    || username.contains('\0')
                    || secret.is_empty()
                    || secret.contains('\0')
                {
                    return Err(SourceCredentialError::InvalidPayload);
                }
            }
            Self::Ssh {
                username,
                private_key,
                passphrase,
            } => {
                if username.trim().is_empty()
                    || username.contains('\0')
                    || private_key.contains('\0')
                    || passphrase
                        .as_deref()
                        .is_some_and(|value| value.contains('\0'))
                    || !private_key.trim().starts_with("-----BEGIN ")
                    || !private_key.trim().ends_with("PRIVATE KEY-----")
                {
                    return Err(SourceCredentialError::InvalidPayload);
                }
            }
        }
        Ok(())
    }

    fn username(&self) -> &str {
        match self {
            Self::Https { username, .. } | Self::Ssh { username, .. } => username,
        }
    }

    fn kind(&self) -> SourceCredentialKind {
        match self {
            Self::Https { .. } => SourceCredentialKind::Https,
            Self::Ssh { .. } => SourceCredentialKind::Ssh,
        }
    }
}

impl From<StoredCredentialPayload> for GitCredential {
    fn from(value: StoredCredentialPayload) -> Self {
        match value {
            StoredCredentialPayload::Https { username, secret } => Self::Https { username, secret },
            StoredCredentialPayload::Ssh {
                username,
                private_key,
                passphrase,
            } => Self::Ssh {
                username,
                private_key,
                passphrase,
            },
        }
    }
}

pub enum CreateSecret {
    Https {
        username: String,
        secret: String,
    },
    Ssh {
        username: String,
        private_key: String,
        passphrase: Option<String>,
    },
}

impl From<CreateSecret> for StoredCredentialPayload {
    fn from(value: CreateSecret) -> Self {
        match value {
            CreateSecret::Https { username, secret } => Self::Https { username, secret },
            CreateSecret::Ssh {
                username,
                private_key,
                passphrase,
            } => Self::Ssh {
                username,
                private_key,
                passphrase,
            },
        }
    }
}

pub struct CreateCredentialParams {
    pub team_id: Uuid,
    pub name: String,
    pub endpoint: RepositoryEndpoint,
    pub secret: CreateSecret,
    pub actor_user_id: Uuid,
}

fn endpoint_kind(
    endpoint: &RepositoryEndpoint,
) -> Result<SourceCredentialKind, SourceCredentialError> {
    match endpoint.transport {
        GitTransport::Https => Ok(SourceCredentialKind::Https),
        GitTransport::Ssh => Ok(SourceCredentialKind::Ssh),
        GitTransport::Http | GitTransport::Git => Err(SourceCredentialError::EndpointMismatch),
    }
}

fn associated_data(
    credential_id: Uuid,
    team_id: Uuid,
    version: i32,
    kind: &SourceCredentialKind,
    host: &str,
    port: i32,
) -> Vec<u8> {
    format!(
        "grass-source-credential:v1:{team_id}:{credential_id}:{version}:{}:{host}:{port}",
        kind.as_str()
    )
    .into_bytes()
}

struct EncryptionScope<'a> {
    credential_id: Uuid,
    team_id: Uuid,
    version: i32,
    kind: &'a SourceCredentialKind,
    host: &'a str,
    port: i32,
}

fn encrypt_payload(
    keyring: &GitCredentialEncryptionConfig,
    scope: EncryptionScope<'_>,
    payload: &StoredCredentialPayload,
) -> Result<(String, serde_json::Value), SourceCredentialError> {
    payload.validate()?;
    if payload.kind() != *scope.kind {
        return Err(SourceCredentialError::EndpointMismatch);
    }
    let (key_id, key) = keyring
        .active_key()
        .map_err(|_| SourceCredentialError::EncryptionUnavailable)?;
    let plaintext =
        serde_json::to_vec(payload).map_err(|_| SourceCredentialError::InvalidPayload)?;
    let envelope = grass_crypto::encrypt_secret(
        key_id,
        &key,
        &plaintext,
        &associated_data(
            scope.credential_id,
            scope.team_id,
            scope.version,
            scope.kind,
            scope.host,
            scope.port,
        ),
    )
    .map_err(|_| SourceCredentialError::EncryptionUnavailable)?;
    let envelope =
        serde_json::to_value(envelope).map_err(|_| SourceCredentialError::InvalidPayload)?;
    Ok((key_id.to_owned(), envelope))
}

pub async fn create(
    db: &DatabaseConnection,
    keyring: &GitCredentialEncryptionConfig,
    params: CreateCredentialParams,
) -> Result<source_credential::Model, SourceCredentialError> {
    let kind = endpoint_kind(&params.endpoint)?;
    let payload: StoredCredentialPayload = params.secret.into();
    let id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let port = i32::from(params.endpoint.port);
    let (key_id, encrypted_payload) = encrypt_payload(
        keyring,
        EncryptionScope {
            credential_id: id,
            team_id: params.team_id,
            version: 1,
            kind: &kind,
            host: &params.endpoint.host,
            port,
        },
        &payload,
    )?;

    let transaction = db.begin().await?;
    source_credential::ActiveModel {
        id: Set(id),
        team_id: Set(params.team_id),
        name: Set(params.name),
        kind: Set(kind.clone()),
        host: Set(params.endpoint.host),
        port: Set(port),
        username: Set(Some(payload.username().to_owned())),
        current_version_id: Set(None),
        revoked_at: Set(None),
        created_by_user_id: Set(Some(params.actor_user_id)),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&transaction)
    .await?;
    source_credential_version::ActiveModel {
        id: Set(version_id),
        credential_id: Set(id),
        version: Set(1),
        key_id: Set(key_id),
        encrypted_payload: Set(encrypted_payload),
        revoked_at: Set(None),
        created_by_user_id: Set(Some(params.actor_user_id)),
        created_at: Set(now),
    }
    .insert(&transaction)
    .await?;
    let mut credential: source_credential::ActiveModel = source_credential::Entity::find_by_id(id)
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::NotFound)?
        .into();
    credential.current_version_id = Set(Some(version_id));
    let credential = credential.update(&transaction).await?;
    transaction.commit().await?;
    Ok(credential)
}

pub async fn list_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
) -> Result<Vec<source_credential::Model>, SourceCredentialError> {
    Ok(source_credential::Entity::find()
        .filter(source_credential::Column::TeamId.eq(team_id))
        .order_by_asc(source_credential::Column::Name)
        .all(db)
        .await?)
}

pub async fn get_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
    credential_id: Uuid,
) -> Result<source_credential::Model, SourceCredentialError> {
    source_credential::Entity::find_by_id(credential_id)
        .filter(source_credential::Column::TeamId.eq(team_id))
        .one(db)
        .await?
        .ok_or(SourceCredentialError::NotFound)
}

pub async fn rotate(
    db: &DatabaseConnection,
    keyring: &GitCredentialEncryptionConfig,
    team_id: Uuid,
    credential_id: Uuid,
    secret: CreateSecret,
    actor_user_id: Uuid,
) -> Result<source_credential::Model, SourceCredentialError> {
    let payload: StoredCredentialPayload = secret.into();
    let transaction = db.begin().await?;
    let credential = source_credential::Entity::find_by_id(credential_id)
        .filter(source_credential::Column::TeamId.eq(team_id))
        .lock_exclusive()
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::NotFound)?;
    if credential.revoked_at.is_some() {
        return Err(SourceCredentialError::Revoked);
    }
    let current = source_credential_version::Entity::find()
        .filter(source_credential_version::Column::CredentialId.eq(credential.id))
        .order_by_desc(source_credential_version::Column::Version)
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::NotFound)?;
    let next_version = current.version + 1;
    let version_id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let (key_id, encrypted_payload) = encrypt_payload(
        keyring,
        EncryptionScope {
            credential_id: credential.id,
            team_id: credential.team_id,
            version: next_version,
            kind: &credential.kind,
            host: &credential.host,
            port: credential.port,
        },
        &payload,
    )?;
    source_credential_version::ActiveModel {
        id: Set(version_id),
        credential_id: Set(credential.id),
        version: Set(next_version),
        key_id: Set(key_id),
        encrypted_payload: Set(encrypted_payload),
        revoked_at: Set(None),
        created_by_user_id: Set(Some(actor_user_id)),
        created_at: Set(now),
    }
    .insert(&transaction)
    .await?;
    let mut active: source_credential::ActiveModel = credential.into();
    active.current_version_id = Set(Some(version_id));
    active.username = Set(Some(payload.username().to_owned()));
    active.updated_at = Set(now);
    let credential = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(credential)
}

pub async fn revoke(
    db: &DatabaseConnection,
    team_id: Uuid,
    credential_id: Uuid,
) -> Result<source_credential::Model, SourceCredentialError> {
    let transaction = db.begin().await?;
    let credential = get_for_team(&transaction, team_id, credential_id).await?;
    let now = OffsetDateTime::now_utc();
    source_credential_version::Entity::update_many()
        .col_expr(
            source_credential_version::Column::RevokedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(source_credential_version::Column::CredentialId.eq(credential.id))
        .filter(source_credential_version::Column::RevokedAt.is_null())
        .exec(&transaction)
        .await?;
    let mut active: source_credential::ActiveModel = credential.into();
    active.revoked_at = Set(Some(now));
    active.updated_at = Set(now);
    let credential = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(credential)
}

fn endpoint_matches(credential: &source_credential::Model, endpoint: &RepositoryEndpoint) -> bool {
    let kind_matches = matches!(
        (&credential.kind, endpoint.transport),
        (SourceCredentialKind::Https, GitTransport::Https)
            | (SourceCredentialKind::Ssh, GitTransport::Ssh)
    );
    kind_matches
        && credential.host.eq_ignore_ascii_case(&endpoint.host)
        && credential.port == i32::from(endpoint.port)
}

pub fn matches_repository_url(credential: &source_credential::Model, repository_url: &str) -> bool {
    grass_git_source::parse_repository_url(repository_url)
        .is_ok_and(|endpoint| endpoint_matches(credential, &endpoint))
}

pub async fn bind_project(
    db: &DatabaseConnection,
    project: &project::Model,
    credential_id: Uuid,
    actor_user_id: Uuid,
) -> Result<source_credential::Model, SourceCredentialError> {
    let repository_url = project
        .repository_url
        .as_deref()
        .ok_or(SourceCredentialError::EndpointMismatch)?;
    let endpoint = grass_git_source::parse_repository_url(repository_url)
        .map_err(|_| SourceCredentialError::EndpointMismatch)?;
    let credential = get_for_team(db, project.team_id, credential_id).await?;
    if credential.revoked_at.is_some() {
        return Err(SourceCredentialError::Revoked);
    }
    if !endpoint_matches(&credential, &endpoint) {
        return Err(SourceCredentialError::EndpointMismatch);
    }
    let now = OffsetDateTime::now_utc();
    if let Some(existing) = project_source_credential::Entity::find_by_id(project.id)
        .one(db)
        .await?
    {
        let mut active: project_source_credential::ActiveModel = existing.into();
        active.credential_id = Set(credential.id);
        active.team_id = Set(project.team_id);
        active.bound_by_user_id = Set(Some(actor_user_id));
        active.updated_at = Set(now);
        active.update(db).await?;
    } else {
        project_source_credential::ActiveModel {
            project_id: Set(project.id),
            credential_id: Set(credential.id),
            team_id: Set(project.team_id),
            bound_by_user_id: Set(Some(actor_user_id)),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await?;
    }
    Ok(credential)
}

pub async fn unbind_project<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<(), SourceCredentialError> {
    project_source_credential::Entity::delete_by_id(project_id)
        .exec(db)
        .await?;
    Ok(())
}

pub async fn bound_credential<C: ConnectionTrait>(
    db: &C,
    project_id: Uuid,
) -> Result<Option<source_credential::Model>, SourceCredentialError> {
    let Some(binding) = project_source_credential::Entity::find_by_id(project_id)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    Ok(source_credential::Entity::find_by_id(binding.credential_id)
        .one(db)
        .await?)
}

pub async fn current_version_for_project<C: ConnectionTrait>(
    db: &C,
    project: &project::Model,
) -> Result<Option<Uuid>, SourceCredentialError> {
    let Some(credential) = bound_credential(db, project.id).await? else {
        return Ok(None);
    };
    let repository_url = project
        .repository_url
        .as_deref()
        .ok_or(SourceCredentialError::EndpointMismatch)?;
    if !matches_repository_url(&credential, repository_url) {
        return Err(SourceCredentialError::EndpointMismatch);
    }
    if credential.revoked_at.is_some() {
        return Err(SourceCredentialError::Revoked);
    }
    let version_id = credential
        .current_version_id
        .ok_or(SourceCredentialError::NotFound)?;
    let version = source_credential_version::Entity::find_by_id(version_id)
        .one(db)
        .await?
        .ok_or(SourceCredentialError::NotFound)?;
    if version.revoked_at.is_some() {
        return Err(SourceCredentialError::Revoked);
    }
    Ok(Some(version.id))
}

pub struct RedeemedSourceCredential {
    pub credential: GitCredential,
    pub host: String,
    pub port: u16,
}

pub async fn issue_lease<C: ConnectionTrait>(
    db: &C,
    node_id: Uuid,
    deployment_id: Uuid,
    credential_version_id: Uuid,
) -> Result<String, SourceCredentialError> {
    let token = grass_token::generate_token();
    let now = OffsetDateTime::now_utc();
    source_credential_lease::ActiveModel {
        id: Set(Uuid::now_v7()),
        token_hash: Set(grass_token::hash_token(&token)),
        node_id: Set(node_id),
        deployment_id: Set(deployment_id),
        credential_version_id: Set(credential_version_id),
        expires_at: Set(now + LEASE_TTL),
        consumed_at: Set(None),
        created_at: Set(now),
    }
    .insert(db)
    .await?;
    Ok(token)
}

fn lease_can_be_redeemed(
    lease: &source_credential_lease::Model,
    node_id: Uuid,
    deployment_id: Uuid,
    now: OffsetDateTime,
) -> bool {
    lease.node_id == node_id
        && lease.deployment_id == deployment_id
        && lease.consumed_at.is_none()
        && lease.expires_at > now
}

fn credential_version_is_active(
    credential: &source_credential::Model,
    version: &source_credential_version::Model,
) -> bool {
    credential.revoked_at.is_none() && version.revoked_at.is_none()
}

pub async fn redeem_lease(
    db: &DatabaseConnection,
    keyring: &GitCredentialEncryptionConfig,
    node_id: Uuid,
    deployment_id: Uuid,
    token: &str,
) -> Result<RedeemedSourceCredential, SourceCredentialError> {
    let transaction = db.begin().await?;
    let lease = source_credential_lease::Entity::find()
        .filter(source_credential_lease::Column::TokenHash.eq(grass_token::hash_token(token)))
        .lock_exclusive()
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::InvalidLease)?;
    let now = OffsetDateTime::now_utc();
    if !lease_can_be_redeemed(&lease, node_id, deployment_id, now) {
        return Err(SourceCredentialError::InvalidLease);
    }
    let version = source_credential_version::Entity::find_by_id(lease.credential_version_id)
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::InvalidLease)?;
    let credential = source_credential::Entity::find_by_id(version.credential_id)
        .one(&transaction)
        .await?
        .ok_or(SourceCredentialError::InvalidLease)?;
    if !credential_version_is_active(&credential, &version) {
        return Err(SourceCredentialError::Revoked);
    }
    let key = keyring
        .key(&version.key_id)
        .map_err(|_| SourceCredentialError::EncryptionUnavailable)?;
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(version.encrypted_payload)
        .map_err(|_| SourceCredentialError::InvalidPayload)?;
    if envelope.key_id != version.key_id {
        return Err(SourceCredentialError::InvalidPayload);
    }
    let plaintext = grass_crypto::decrypt_secret(
        &envelope,
        &key,
        &associated_data(
            credential.id,
            credential.team_id,
            version.version,
            &credential.kind,
            &credential.host,
            credential.port,
        ),
    )
    .map_err(|_| SourceCredentialError::InvalidPayload)?;
    let payload: StoredCredentialPayload =
        serde_json::from_slice(&plaintext).map_err(|_| SourceCredentialError::InvalidPayload)?;
    payload.validate()?;

    let mut active: source_credential_lease::ActiveModel = lease.into();
    active.consumed_at = Set(Some(now));
    active.update(&transaction).await?;
    transaction.commit().await?;
    let port = u16::try_from(credential.port).map_err(|_| SourceCredentialError::InvalidPayload)?;
    Ok(RedeemedSourceCredential {
        credential: payload.into(),
        host: credential.host,
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(kind: SourceCredentialKind, host: &str, port: i32) -> source_credential::Model {
        let now = OffsetDateTime::UNIX_EPOCH;
        source_credential::Model {
            id: Uuid::nil(),
            team_id: Uuid::nil(),
            name: "test".to_owned(),
            kind,
            host: host.to_owned(),
            port,
            username: Some("git".to_owned()),
            current_version_id: None,
            revoked_at: None,
            created_by_user_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn lease(now: OffsetDateTime) -> source_credential_lease::Model {
        source_credential_lease::Model {
            id: Uuid::now_v7(),
            token_hash: "0".repeat(64),
            node_id: Uuid::now_v7(),
            deployment_id: Uuid::now_v7(),
            credential_version_id: Uuid::now_v7(),
            expires_at: now + Duration::minutes(5),
            consumed_at: None,
            created_at: now,
        }
    }

    #[test]
    fn credential_scope_matches_scheme_host_and_port() {
        let endpoint =
            grass_git_source::parse_repository_url("ssh://git@EXAMPLE.com:2222/org/repo.git")
                .unwrap();
        assert!(endpoint_matches(
            &credential(SourceCredentialKind::Ssh, "example.com", 2222),
            &endpoint
        ));
        assert!(!endpoint_matches(
            &credential(SourceCredentialKind::Https, "example.com", 2222),
            &endpoint
        ));
        assert!(!endpoint_matches(
            &credential(SourceCredentialKind::Ssh, "example.com", 22),
            &endpoint
        ));
    }

    #[test]
    fn lease_rejects_replay_expiry_and_binding_mismatches() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let valid = lease(now);
        assert!(lease_can_be_redeemed(
            &valid,
            valid.node_id,
            valid.deployment_id,
            now
        ));
        assert!(!lease_can_be_redeemed(
            &valid,
            Uuid::now_v7(),
            valid.deployment_id,
            now
        ));
        assert!(!lease_can_be_redeemed(
            &valid,
            valid.node_id,
            Uuid::now_v7(),
            now
        ));

        let mut expired = valid.clone();
        expired.expires_at = now;
        assert!(!lease_can_be_redeemed(
            &expired,
            expired.node_id,
            expired.deployment_id,
            now
        ));

        let mut replayed = valid;
        replayed.consumed_at = Some(now);
        assert!(!lease_can_be_redeemed(
            &replayed,
            replayed.node_id,
            replayed.deployment_id,
            now
        ));
    }

    #[test]
    fn credential_or_version_revocation_blocks_redemption() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let active_credential = credential(SourceCredentialKind::Https, "example.com", 443);
        let active_version = source_credential_version::Model {
            id: Uuid::now_v7(),
            credential_id: active_credential.id,
            version: 1,
            key_id: "v1".to_owned(),
            encrypted_payload: serde_json::json!({}),
            revoked_at: None,
            created_by_user_id: None,
            created_at: now,
        };
        assert!(credential_version_is_active(
            &active_credential,
            &active_version
        ));

        let mut revoked_version = active_version.clone();
        revoked_version.revoked_at = Some(now);
        assert!(!credential_version_is_active(
            &active_credential,
            &revoked_version
        ));

        let mut revoked_credential = active_credential;
        revoked_credential.revoked_at = Some(now);
        assert!(!credential_version_is_active(
            &revoked_credential,
            &active_version
        ));
    }
}
