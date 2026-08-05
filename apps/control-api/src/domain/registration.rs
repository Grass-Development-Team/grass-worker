use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QuerySelect};
use uuid::Uuid;

use crate::{
    domain::codes::{self, CodeScope, CodeUseError},
    infra::database::entity::{code, registration_email_allowlist},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignupPolicy {
    Open,
    InviteOnly,
    Closed,
}

impl SignupPolicy {
    pub fn parse(value: Option<&str>) -> Result<Self, RegistrationAccessError> {
        match value.unwrap_or("open") {
            "open" => Ok(Self::Open),
            "invite_only" => Ok(Self::InviteOnly),
            "closed" => Ok(Self::Closed),
            _ => Err(RegistrationAccessError::InvalidPolicy),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InviteOnly => "invite_only",
            Self::Closed => "closed",
        }
    }
}

pub enum RegistrationGrant {
    Open,
    Code(code::Model),
    Email(registration_email_allowlist::Model),
}

#[derive(Debug, thiserror::Error)]
pub enum RegistrationAccessError {
    #[error("signup policy setting is invalid")]
    InvalidPolicy,
    #[error("user registration is closed")]
    Closed,
    #[error("a registration code or allowed email is required")]
    CredentialRequired,
    #[error(transparent)]
    Code(#[from] CodeUseError),
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

pub async fn authorize_registration<C: ConnectionTrait>(
    db: &C,
    policy: SignupPolicy,
    email: &str,
    registration_code: Option<&str>,
) -> Result<RegistrationGrant, RegistrationAccessError> {
    match policy {
        SignupPolicy::Open => Ok(RegistrationGrant::Open),
        SignupPolicy::Closed => Err(RegistrationAccessError::Closed),
        SignupPolicy::InviteOnly => {
            if let Some(value) = registration_code
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                return codes::lock_code_for_redemption(db, value, CodeScope::Registration)
                    .await
                    .map(RegistrationGrant::Code)
                    .map_err(Into::into);
            }
            registration_email_allowlist::Entity::find()
                .filter(registration_email_allowlist::Column::Email.eq(email))
                .lock_exclusive()
                .one(db)
                .await
                .map_err(|source| RegistrationAccessError::Database(source.into()))?
                .map(RegistrationGrant::Email)
                .ok_or(RegistrationAccessError::CredentialRequired)
        }
    }
}

pub async fn consume_registration_grant<C: ConnectionTrait>(
    db: &C,
    grant: RegistrationGrant,
    user_id: Uuid,
) -> Result<(), RegistrationAccessError> {
    match grant {
        RegistrationGrant::Open => Ok(()),
        RegistrationGrant::Code(code) => {
            codes::consume_locked_code(db, code, user_id).await?;
            Ok(())
        }
        RegistrationGrant::Email(entry) => {
            registration_email_allowlist::Entity::delete_by_id(entry.id)
                .exec(db)
                .await
                .map_err(|source| RegistrationAccessError::Database(source.into()))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, MockDatabase, MockExecResult};
    use time::OffsetDateTime;
    use uuid::Uuid;

    use super::*;
    use crate::infra::database::entity::registration_email_allowlist;

    #[test]
    fn parses_the_three_signup_policies() {
        assert_eq!(SignupPolicy::parse(None).unwrap(), SignupPolicy::Open);
        assert_eq!(
            SignupPolicy::parse(Some("open")).unwrap(),
            SignupPolicy::Open
        );
        assert_eq!(
            SignupPolicy::parse(Some("invite_only")).unwrap(),
            SignupPolicy::InviteOnly
        );
        assert_eq!(
            SignupPolicy::parse(Some("closed")).unwrap(),
            SignupPolicy::Closed
        );
        assert!(SignupPolicy::parse(Some("invalid")).is_err());
    }

    #[tokio::test]
    async fn open_registration_needs_no_credential_and_closed_registration_rejects_all() {
        let database = MockDatabase::new(DbBackend::Postgres).into_connection();

        assert!(matches!(
            authorize_registration(&database, SignupPolicy::Open, "user@example.com", None,)
                .await
                .unwrap(),
            RegistrationGrant::Open
        ));
        assert!(matches!(
            authorize_registration(
                &database,
                SignupPolicy::Closed,
                "user@example.com",
                Some("unused"),
            )
            .await,
            Err(RegistrationAccessError::Closed)
        ));
        assert!(database.into_transaction_log().is_empty());
    }

    #[tokio::test]
    async fn invite_only_locks_and_consumes_an_exact_email_entry() {
        let entry = registration_email_allowlist::Model {
            id: Uuid::now_v7(),
            email: "user@example.com".to_owned(),
            created_by_user_id: Some(Uuid::now_v7()),
            created_at: OffsetDateTime::now_utc(),
        };
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[entry.clone()]])
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();

        let grant = authorize_registration(
            &database,
            SignupPolicy::InviteOnly,
            "user@example.com",
            None,
        )
        .await
        .unwrap();
        assert!(matches!(&grant, RegistrationGrant::Email(item) if item.id == entry.id));
        consume_registration_grant(&database, grant, Uuid::now_v7())
            .await
            .unwrap();
        let statements = format!("{:?}", database.into_transaction_log());

        assert!(statements.contains("FOR UPDATE"));
        assert!(statements.contains("DELETE FROM \\\"registration_email_allowlist\\\""));
        assert!(!statements.contains("INSERT INTO"));
    }

    #[tokio::test]
    async fn invite_only_rejects_an_email_without_code_or_allowlist_entry() {
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([Vec::<registration_email_allowlist::Model>::new()])
            .into_connection();

        assert!(matches!(
            authorize_registration(
                &database,
                SignupPolicy::InviteOnly,
                "missing@example.com",
                None,
            )
            .await,
            Err(RegistrationAccessError::CredentialRequired)
        ));
    }
}
