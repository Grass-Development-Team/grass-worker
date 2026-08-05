use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::infra::database::entity::code;

pub const CODE_LENGTH: usize = 40;
pub const CODE_PREFIX_LENGTH: usize = 6;
pub const CODE_SUFFIX_LENGTH: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeScope {
    Registration,
}

impl CodeScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "registration" => Some(Self::Registration),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
        }
    }
}

const REGISTERED_SCOPES: [CodeScope; 1] = [CodeScope::Registration];

pub const fn registered_scopes() -> &'static [CodeScope] {
    &REGISTERED_SCOPES
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodeStatus {
    Available,
    Used,
    Expired,
    Revoked,
}

impl CodeStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "available" => Some(Self::Available),
            "used" => Some(Self::Used),
            "expired" => Some(Self::Expired),
            "revoked" => Some(Self::Revoked),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Used => "used",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }
}

pub fn generate_code_value() -> String {
    grass_token::generate_token()[..CODE_LENGTH].to_owned()
}

pub fn code_prefix(value: &str) -> &str {
    &value[..CODE_PREFIX_LENGTH]
}

pub fn code_suffix(value: &str) -> &str {
    &value[value.len() - CODE_SUFFIX_LENGTH..]
}

pub fn lifecycle_status(
    used_at: Option<OffsetDateTime>,
    revoked_at: Option<OffsetDateTime>,
    expires_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> CodeStatus {
    if used_at.is_some() {
        CodeStatus::Used
    } else if revoked_at.is_some() {
        CodeStatus::Revoked
    } else if expires_at.is_some_and(|expires_at| expires_at <= now) {
        CodeStatus::Expired
    } else {
        CodeStatus::Available
    }
}

pub struct GeneratedCode {
    pub value: String,
    pub model: code::Model,
}

pub fn prepare_code(
    scope: CodeScope,
    expires_at: Option<OffsetDateTime>,
    created_by_user_id: Option<Uuid>,
) -> GeneratedCode {
    let value = generate_code_value();
    let model = code::Model {
        id: Uuid::now_v7(),
        scope: scope.as_str().to_owned(),
        token_hash: grass_token::hash_token(&value),
        token_prefix: code_prefix(&value).to_owned(),
        token_suffix: code_suffix(&value).to_owned(),
        expires_at,
        used_at: None,
        used_by_user_id: None,
        revoked_at: None,
        created_by_user_id,
        created_at: OffsetDateTime::now_utc(),
    };
    GeneratedCode { value, model }
}

#[derive(Debug, thiserror::Error)]
pub enum CodeUseError {
    #[error("code was not found")]
    NotFound,
    #[error("code belongs to a different scope")]
    WrongScope,
    #[error("code has already been used")]
    Used,
    #[error("code has expired")]
    Expired,
    #[error("code has been revoked")]
    Revoked,
    #[error(transparent)]
    Database(#[from] anyhow::Error),
}

pub fn validate_redemption(
    code: &code::Model,
    expected_scope: CodeScope,
    now: OffsetDateTime,
) -> Result<(), CodeUseError> {
    if code.scope != expected_scope.as_str() {
        return Err(CodeUseError::WrongScope);
    }
    match lifecycle_status(code.used_at, code.revoked_at, code.expires_at, now) {
        CodeStatus::Available => Ok(()),
        CodeStatus::Used => Err(CodeUseError::Used),
        CodeStatus::Expired => Err(CodeUseError::Expired),
        CodeStatus::Revoked => Err(CodeUseError::Revoked),
    }
}

pub fn query_codes(
    scope: Option<CodeScope>,
    status: Option<CodeStatus>,
    now: OffsetDateTime,
) -> sea_orm::Select<code::Entity> {
    let mut query = code::Entity::find();
    if let Some(scope) = scope {
        query = query.filter(code::Column::Scope.eq(scope.as_str()));
    }
    query = match status {
        Some(CodeStatus::Available) => query
            .filter(code::Column::UsedAt.is_null())
            .filter(code::Column::RevokedAt.is_null())
            .filter(
                sea_orm::Condition::any()
                    .add(code::Column::ExpiresAt.is_null())
                    .add(code::Column::ExpiresAt.gt(now)),
            ),
        Some(CodeStatus::Used) => query.filter(code::Column::UsedAt.is_not_null()),
        Some(CodeStatus::Expired) => query
            .filter(code::Column::UsedAt.is_null())
            .filter(code::Column::RevokedAt.is_null())
            .filter(code::Column::ExpiresAt.lte(now)),
        Some(CodeStatus::Revoked) => query.filter(code::Column::RevokedAt.is_not_null()),
        None => query,
    };
    query
        .order_by_desc(code::Column::CreatedAt)
        .order_by_desc(code::Column::Id)
}

pub async fn generate_codes<C: ConnectionTrait>(
    db: &C,
    scope: CodeScope,
    count: usize,
    expires_at: Option<OffsetDateTime>,
    created_by_user_id: Option<Uuid>,
) -> anyhow::Result<Vec<GeneratedCode>> {
    let generated = (0..count)
        .map(|_| prepare_code(scope, expires_at, created_by_user_id))
        .collect::<Vec<_>>();
    code::Entity::insert_many(
        generated
            .iter()
            .map(|item| code::ActiveModel::from(item.model.clone())),
    )
    .exec_without_returning(db)
    .await?;
    Ok(generated)
}

#[allow(dead_code)]
pub async fn redeem_code<C: ConnectionTrait>(
    db: &C,
    value: &str,
    expected_scope: CodeScope,
    user_id: Uuid,
) -> Result<code::Model, CodeUseError> {
    let token_hash = grass_token::hash_token(value.trim());
    let code = code::Entity::find()
        .filter(code::Column::TokenHash.eq(token_hash))
        .lock_exclusive()
        .one(db)
        .await
        .map_err(|source| CodeUseError::Database(source.into()))?
        .ok_or(CodeUseError::NotFound)?;
    let now = OffsetDateTime::now_utc();
    validate_redemption(&code, expected_scope, now)?;

    let mut active: code::ActiveModel = code.into();
    active.used_at = Set(Some(now));
    active.used_by_user_id = Set(Some(user_id));
    active
        .update(db)
        .await
        .map_err(|source| CodeUseError::Database(source.into()))
}

pub async fn revoke_code<C: ConnectionTrait>(
    db: &C,
    code_id: Uuid,
) -> Result<code::Model, CodeUseError> {
    let code = code::Entity::find_by_id(code_id)
        .lock_exclusive()
        .one(db)
        .await
        .map_err(|source| CodeUseError::Database(source.into()))?
        .ok_or(CodeUseError::NotFound)?;
    validate_redemption(
        &code,
        CodeScope::parse(&code.scope).ok_or(CodeUseError::WrongScope)?,
        OffsetDateTime::now_utc(),
    )?;

    let mut active: code::ActiveModel = code.into();
    active.revoked_at = Set(Some(OffsetDateTime::now_utc()));
    active
        .update(db)
        .await
        .map_err(|source| CodeUseError::Database(source.into()))
}

#[cfg(test)]
mod tests {
    use sea_orm::{DbBackend, MockDatabase, MockExecResult};
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn only_backend_registered_scopes_are_accepted() {
        assert_eq!(
            CodeScope::parse("registration"),
            Some(CodeScope::Registration)
        );
        assert_eq!(CodeScope::Registration.as_str(), "registration");
        assert_eq!(registered_scopes(), &[CodeScope::Registration]);
        assert_eq!(CodeScope::parse("discount"), None);
    }

    #[test]
    fn generated_codes_have_the_approved_shape() {
        let generated = generate_code_value();

        assert_eq!(generated.len(), CODE_LENGTH);
        assert_eq!(generated.len(), 40);
        assert!(
            generated.chars().all(
                |character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            )
        );
        assert_eq!(code_prefix(&generated), &generated[..6]);
        assert_eq!(code_suffix(&generated), &generated[generated.len() - 4..]);
    }

    #[test]
    fn lifecycle_status_has_a_stable_precedence() {
        let now = OffsetDateTime::now_utc();

        assert_eq!(
            lifecycle_status(Some(now), Some(now), Some(now - Duration::days(1)), now),
            CodeStatus::Used
        );
        assert_eq!(
            lifecycle_status(None, Some(now), Some(now - Duration::days(1)), now),
            CodeStatus::Revoked
        );
        assert_eq!(
            lifecycle_status(None, None, Some(now), now),
            CodeStatus::Expired
        );
        assert_eq!(
            lifecycle_status(None, None, Some(now + Duration::seconds(1)), now),
            CodeStatus::Available
        );
        assert_eq!(
            lifecycle_status(None, None, None, now),
            CodeStatus::Available
        );
    }

    #[test]
    fn prepared_codes_persist_only_hash_and_preview() {
        let creator_id = Uuid::now_v7();
        let expires_at = OffsetDateTime::now_utc() + Duration::days(30);
        let generated = prepare_code(CodeScope::Registration, Some(expires_at), Some(creator_id));

        assert_eq!(generated.value.len(), 40);
        assert_eq!(generated.model.scope, "registration");
        assert_eq!(
            generated.model.token_hash,
            grass_token::hash_token(&generated.value)
        );
        assert_ne!(generated.model.token_hash, generated.value);
        assert_eq!(generated.model.token_prefix, generated.value[..6]);
        assert_eq!(generated.model.token_suffix, generated.value[36..]);
        assert_eq!(generated.model.expires_at, Some(expires_at));
        assert_eq!(generated.model.created_by_user_id, Some(creator_id));
        assert!(generated.model.used_at.is_none());
        assert!(generated.model.used_by_user_id.is_none());
        assert!(generated.model.revoked_at.is_none());
    }

    #[test]
    fn redemption_validation_enforces_scope_and_single_use() {
        let now = OffsetDateTime::now_utc();
        let generated = prepare_code(
            CodeScope::Registration,
            Some(now + Duration::days(30)),
            None,
        );

        assert!(validate_redemption(&generated.model, CodeScope::Registration, now).is_ok());

        let mut wrong_scope = generated.model.clone();
        wrong_scope.scope = "discount".to_owned();
        assert!(matches!(
            validate_redemption(&wrong_scope, CodeScope::Registration, now),
            Err(CodeUseError::WrongScope)
        ));

        let mut used = generated.model.clone();
        used.used_at = Some(now);
        assert!(matches!(
            validate_redemption(&used, CodeScope::Registration, now),
            Err(CodeUseError::Used)
        ));

        let mut revoked = generated.model.clone();
        revoked.revoked_at = Some(now);
        assert!(matches!(
            validate_redemption(&revoked, CodeScope::Registration, now),
            Err(CodeUseError::Revoked)
        ));

        let mut expired = generated.model;
        expired.expires_at = Some(now);
        assert!(matches!(
            validate_redemption(&expired, CodeScope::Registration, now),
            Err(CodeUseError::Expired)
        ));
    }

    #[tokio::test]
    async fn batch_generation_never_inserts_plaintext_values() {
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 2,
            }])
            .into_connection();

        let generated = generate_codes(
            &database,
            CodeScope::Registration,
            2,
            None,
            Some(Uuid::now_v7()),
        )
        .await
        .unwrap();
        let statements = format!("{:?}", database.into_transaction_log());

        assert!(statements.contains("INSERT INTO \\\"codes\\\""));
        for item in generated {
            assert!(!statements.contains(&item.value));
            assert!(statements.contains(&item.model.token_hash));
        }
    }

    #[tokio::test]
    async fn redemption_locks_the_code_and_records_the_user() {
        let user_id = Uuid::now_v7();
        let generated = prepare_code(CodeScope::Registration, None, None);
        let mut redeemed = generated.model.clone();
        redeemed.used_at = Some(OffsetDateTime::now_utc());
        redeemed.used_by_user_id = Some(user_id);
        let database = MockDatabase::new(DbBackend::Postgres)
            .append_query_results([[generated.model], [redeemed.clone()]])
            .into_connection();

        let result = redeem_code(
            &database,
            &generated.value,
            CodeScope::Registration,
            user_id,
        )
        .await
        .unwrap();
        let statements = format!("{:?}", database.into_transaction_log());

        assert_eq!(result.used_by_user_id, Some(user_id));
        assert!(result.used_at.is_some());
        assert!(statements.contains("FOR UPDATE"));
        assert!(statements.contains("UPDATE \\\"codes\\\""));
    }
}
