use base64::{Engine, engine::general_purpose::STANDARD};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::{SshHostKeyStatus, ssh_host_key};

#[derive(Debug, thiserror::Error)]
pub enum SshHostKeyError {
    #[error("SSH host key was not found")]
    NotFound,
    #[error("SSH host key payload is invalid")]
    Invalid,
    #[error(transparent)]
    Database(#[from] sea_orm::DbErr),
}

pub struct ObserveHostKeyParams {
    pub team_id: Uuid,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint_sha256: String,
    pub node_id: Uuid,
}

pub fn fingerprint(public_key: &str) -> Result<String, SshHostKeyError> {
    let bytes = STANDARD
        .decode(public_key)
        .map_err(|_| SshHostKeyError::Invalid)?;
    if bytes.is_empty() {
        return Err(SshHostKeyError::Invalid);
    }
    Ok(format!(
        "SHA256:{}",
        STANDARD.encode(Sha256::digest(bytes)).trim_end_matches('=')
    ))
}

pub fn known_hosts_line(key: &ssh_host_key::Model) -> String {
    let host = if key.port == 22 {
        key.host.clone()
    } else {
        format!("[{}]:{}", key.host, key.port)
    };
    format!("{host} {} {}", key.key_type, key.public_key)
}

pub async fn observe<C: ConnectionTrait>(
    db: &C,
    params: ObserveHostKeyParams,
) -> Result<ssh_host_key::Model, SshHostKeyError> {
    if params.host.trim().is_empty()
        || params.key_type.trim().is_empty()
        || params.port == 0
        || fingerprint(&params.public_key)? != params.fingerprint_sha256
    {
        return Err(SshHostKeyError::Invalid);
    }
    let now = OffsetDateTime::now_utc();
    if let Some(existing) = ssh_host_key::Entity::find()
        .filter(ssh_host_key::Column::TeamId.eq(params.team_id))
        .filter(ssh_host_key::Column::Host.eq(&params.host))
        .filter(ssh_host_key::Column::Port.eq(i32::from(params.port)))
        .filter(ssh_host_key::Column::FingerprintSha256.eq(&params.fingerprint_sha256))
        .one(db)
        .await?
    {
        let mut active: ssh_host_key::ActiveModel = existing.into();
        active.last_seen_at = Set(now);
        active.updated_at = Set(now);
        return Ok(active.update(db).await?);
    }
    Ok(ssh_host_key::ActiveModel {
        id: Set(Uuid::now_v7()),
        team_id: Set(params.team_id),
        host: Set(params.host),
        port: Set(i32::from(params.port)),
        key_type: Set(params.key_type),
        public_key: Set(params.public_key),
        fingerprint_sha256: Set(params.fingerprint_sha256),
        status: Set(SshHostKeyStatus::Pending),
        first_seen_node_id: Set(Some(params.node_id)),
        approved_by_user_id: Set(None),
        approved_at: Set(None),
        last_seen_at: Set(now),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await?)
}

pub async fn list_for_team<C: ConnectionTrait>(
    db: &C,
    team_id: Uuid,
) -> Result<Vec<ssh_host_key::Model>, SshHostKeyError> {
    Ok(ssh_host_key::Entity::find()
        .filter(ssh_host_key::Column::TeamId.eq(team_id))
        .order_by_desc(ssh_host_key::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn set_status(
    db: &DatabaseConnection,
    team_id: Uuid,
    key_id: Uuid,
    status: SshHostKeyStatus,
    actor_user_id: Uuid,
) -> Result<ssh_host_key::Model, SshHostKeyError> {
    let transaction = db.begin().await?;
    let key = ssh_host_key::Entity::find_by_id(key_id)
        .filter(ssh_host_key::Column::TeamId.eq(team_id))
        .lock_exclusive()
        .one(&transaction)
        .await?
        .ok_or(SshHostKeyError::NotFound)?;
    let now = OffsetDateTime::now_utc();
    if status == SshHostKeyStatus::Approved {
        ssh_host_key::Entity::update_many()
            .col_expr(
                ssh_host_key::Column::Status,
                sea_orm::ActiveEnum::as_enum(&SshHostKeyStatus::Superseded),
            )
            .col_expr(
                ssh_host_key::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(ssh_host_key::Column::TeamId.eq(team_id))
            .filter(ssh_host_key::Column::Host.eq(&key.host))
            .filter(ssh_host_key::Column::Port.eq(key.port))
            .filter(ssh_host_key::Column::Status.eq(SshHostKeyStatus::Approved))
            .filter(ssh_host_key::Column::Id.ne(key.id))
            .exec(&transaction)
            .await?;
    }
    let mut active: ssh_host_key::ActiveModel = key.into();
    active.status = Set(status.clone());
    active.updated_at = Set(now);
    if status == SshHostKeyStatus::Approved {
        active.approved_by_user_id = Set(Some(actor_user_id));
        active.approved_at = Set(Some(now));
    } else {
        active.approved_by_user_id = Set(None);
        active.approved_at = Set(None);
    }
    let key = active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_and_known_hosts_lines_are_stable() {
        let public_key = STANDARD.encode(b"test-public-key");
        assert!(fingerprint(&public_key).unwrap().starts_with("SHA256:"));
        assert!(fingerprint("not base64").is_err());

        let now = OffsetDateTime::UNIX_EPOCH;
        let key = ssh_host_key::Model {
            id: Uuid::nil(),
            team_id: Uuid::nil(),
            host: "git.example.com".to_owned(),
            port: 2222,
            key_type: "ssh-ed25519".to_owned(),
            public_key: public_key.clone(),
            fingerprint_sha256: fingerprint(&public_key).unwrap(),
            status: SshHostKeyStatus::Approved,
            first_seen_node_id: None,
            approved_by_user_id: None,
            approved_at: None,
            last_seen_at: now,
            created_at: now,
            updated_at: now,
        };
        assert_eq!(
            known_hosts_line(&key),
            format!("[git.example.com]:2222 ssh-ed25519 {public_key}")
        );
    }
}
