use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    domain::settings,
    infra::database::entity::{
        AuthTokenKind, MfaFactorKind, PlatformRole, user, user_auth_token, user_mfa_factor,
        user_password_credential, user_password_history,
    },
};

pub const PASSWORD_POLICY_KEY: &str = "auth.password_policy";
pub const REGISTRATION_VERIFICATION_KEY: &str = "auth.registration_email_verification";
pub const MFA_POLICY_KEY: &str = "auth.mfa_policy";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct PasswordPolicy {
    pub min_length: usize,
    pub max_length: usize,
    pub require_lowercase: bool,
    pub require_uppercase: bool,
    pub require_number: bool,
    pub require_symbol: bool,
    pub history_count: usize,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_length: 8,
            max_length: 1024,
            require_lowercase: false,
            require_uppercase: false,
            require_number: false,
            require_symbol: false,
            history_count: 0,
        }
    }
}

impl PasswordPolicy {
    pub fn validate_config(&self) -> Result<(), &'static str> {
        if self.min_length < 8 || self.min_length > 128 {
            return Err("password minimum length must be between 8 and 128");
        }
        if self.max_length < self.min_length || self.max_length > 1024 {
            return Err("password maximum length must be between the minimum and 1024");
        }
        if self.history_count > 20 {
            return Err("password history count must not exceed 20");
        }
        Ok(())
    }

    pub fn validate_password(&self, password: &str) -> Result<(), &'static str> {
        self.validate_config()?;
        let length = password.chars().count();
        if length < self.min_length || password.len() > self.max_length {
            return Err("password does not satisfy the configured length policy");
        }
        if self.require_lowercase && !password.chars().any(char::is_lowercase) {
            return Err("password must contain a lowercase letter");
        }
        if self.require_uppercase && !password.chars().any(char::is_uppercase) {
            return Err("password must contain an uppercase letter");
        }
        if self.require_number && !password.chars().any(|character| character.is_ascii_digit()) {
            return Err("password must contain a number");
        }
        if self.require_symbol
            && !password
                .chars()
                .any(|character| !character.is_alphanumeric())
        {
            return Err("password must contain a symbol");
        }
        Ok(())
    }

    pub fn generate_password(&self) -> Result<String, &'static str> {
        self.validate_config()?;
        let target_length = self.min_length.max(24).min(self.max_length);
        let mut password = String::with_capacity(target_length);
        if self.require_lowercase {
            password.push('a');
        }
        if self.require_uppercase {
            password.push('A');
        }
        if self.require_number {
            password.push('1');
        }
        if self.require_symbol {
            password.push('!');
        }
        while password.len() < target_length {
            let token = grass_token::generate_token();
            let remaining = target_length - password.len();
            password.extend(token.chars().take(remaining));
        }
        debug_assert!(self.validate_password(&password).is_ok());
        Ok(password)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MfaEnforcement {
    #[default]
    None,
    PlatformAdmins,
    AllUsers,
    SelectedUsers,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct MfaPolicy {
    pub allowed_factors: Vec<String>,
    pub enforcement: MfaEnforcement,
    pub selected_user_ids: Vec<Uuid>,
}

impl Default for MfaPolicy {
    fn default() -> Self {
        Self {
            allowed_factors: vec!["totp".to_owned()],
            enforcement: MfaEnforcement::None,
            selected_user_ids: Vec::new(),
        }
    }
}

impl MfaPolicy {
    pub fn validate(&self, mail_enabled: bool) -> Result<(), &'static str> {
        if self.allowed_factors.len() > 2 {
            return Err("allowed MFA factors must not contain duplicates");
        }
        if self.allowed_factors.is_empty() && !matches!(self.enforcement, MfaEnforcement::None) {
            return Err("enforced MFA requires at least one allowed factor");
        }
        for factor in &self.allowed_factors {
            if MfaFactorKind::parse(factor).is_none() {
                return Err("allowed MFA factors must be totp or email");
            }
        }
        if self
            .allowed_factors
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != self.allowed_factors.len()
        {
            return Err("allowed MFA factors must not contain duplicates");
        }
        if self.allowed_factors.iter().any(|factor| factor == "email") && !mail_enabled {
            return Err("email MFA requires an enabled mail transport");
        }
        if !matches!(self.enforcement, MfaEnforcement::SelectedUsers)
            && !self.selected_user_ids.is_empty()
        {
            return Err("selected MFA users require selected_users enforcement");
        }
        if self.selected_user_ids.len() > 1_000 {
            return Err("selected MFA users must not exceed 1000 entries");
        }
        if self
            .selected_user_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != self.selected_user_ids.len()
        {
            return Err("selected MFA users must not contain duplicates");
        }
        Ok(())
    }

    pub fn allows(&self, kind: &MfaFactorKind) -> bool {
        self.allowed_factors
            .iter()
            .any(|allowed| allowed == kind.as_str())
    }

    pub fn requires(&self, user_id: Uuid, role: &PlatformRole) -> bool {
        match self.enforcement {
            MfaEnforcement::None => false,
            MfaEnforcement::PlatformAdmins => matches!(role, PlatformRole::Admin),
            MfaEnforcement::AllUsers => true,
            MfaEnforcement::SelectedUsers => self.selected_user_ids.contains(&user_id),
        }
    }
}

pub async fn password_policy(db: &DatabaseConnection) -> anyhow::Result<PasswordPolicy> {
    load_json_setting(db, PASSWORD_POLICY_KEY).await
}

pub async fn mfa_policy(db: &DatabaseConnection) -> anyhow::Result<MfaPolicy> {
    load_json_setting(db, MFA_POLICY_KEY).await
}

pub async fn selected_mfa_users_exist(
    db: &DatabaseConnection,
    user_ids: &[Uuid],
) -> anyhow::Result<bool> {
    if user_ids.is_empty() {
        return Ok(true);
    }
    Ok(user::Entity::find()
        .filter(user::Column::Id.is_in(user_ids.iter().copied()))
        .count(db)
        .await?
        == user_ids.len() as u64)
}

pub async fn registration_verification_required(db: &DatabaseConnection) -> anyhow::Result<bool> {
    Ok(settings::get_setting(db, REGISTRATION_VERIFICATION_KEY)
        .await?
        .and_then(|setting| setting.value.as_bool())
        .unwrap_or(false))
}

async fn load_json_setting<T>(db: &DatabaseConnection, key: &str) -> anyhow::Result<T>
where
    T: Default + serde::de::DeserializeOwned,
{
    settings::get_setting(db, key)
        .await?
        .map(|setting| serde_json::from_value(setting.value).map_err(Into::into))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub async fn password_was_used_recently(
    db: &DatabaseConnection,
    user_id: Uuid,
    password: &str,
    count: usize,
) -> anyhow::Result<bool> {
    if count == 0 {
        return Ok(false);
    }
    let history = user_password_history::Entity::find()
        .filter(user_password_history::Column::UserId.eq(user_id))
        .order_by_desc(user_password_history::Column::CreatedAt)
        .order_by_desc(user_password_history::Column::Id)
        .limit(count as u64)
        .all(db)
        .await?;
    Ok(history.iter().any(|entry| {
        grass_crypto::verify_password(password, &entry.password_hash).unwrap_or(false)
    }))
}

pub async fn verify_password_for_user(
    db: &DatabaseConnection,
    user_id: Uuid,
    password: &str,
) -> anyhow::Result<bool> {
    let credential = user_password_credential::Entity::find()
        .filter(user_password_credential::Column::UserId.eq(user_id))
        .one(db)
        .await?;
    Ok(credential.is_some_and(|credential| {
        grass_crypto::verify_password(password, &credential.password_hash).unwrap_or(false)
    }))
}

pub async fn create_auth_token(
    db: &DatabaseConnection,
    user_id: Uuid,
    kind: AuthTokenKind,
    ttl: Duration,
) -> anyhow::Result<String> {
    let now = OffsetDateTime::now_utc();
    let transaction = db.begin().await?;
    user_auth_token::Entity::update_many()
        .col_expr(
            user_auth_token::Column::UsedAt,
            sea_orm::sea_query::Expr::value(now),
        )
        .filter(user_auth_token::Column::UserId.eq(user_id))
        .filter(user_auth_token::Column::Kind.eq(kind.clone()))
        .filter(user_auth_token::Column::UsedAt.is_null())
        .exec(&transaction)
        .await?;
    let token = grass_token::generate_token();
    user_auth_token::ActiveModel {
        id: Set(Uuid::now_v7()),
        user_id: Set(user_id),
        kind: Set(kind),
        token_hash: Set(grass_token::hash_token(&token)),
        expires_at: Set(now + ttl),
        used_at: Set(None),
        created_at: Set(now),
    }
    .insert(&transaction)
    .await?;
    transaction.commit().await?;
    Ok(token)
}

pub async fn consume_auth_token(
    db: &DatabaseConnection,
    token: &str,
    kind: AuthTokenKind,
) -> anyhow::Result<Option<Uuid>> {
    let transaction = db.begin().await?;
    let record = user_auth_token::Entity::find()
        .filter(user_auth_token::Column::TokenHash.eq(grass_token::hash_token(token)))
        .filter(user_auth_token::Column::Kind.eq(kind))
        .lock_exclusive()
        .one(&transaction)
        .await?;
    let Some(record) = record
        .filter(|record| record.used_at.is_none() && record.expires_at > OffsetDateTime::now_utc())
    else {
        transaction.rollback().await?;
        return Ok(None);
    };
    let user_id = record.user_id;
    let mut active: user_auth_token::ActiveModel = record.into();
    active.used_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(&transaction).await?;
    transaction.commit().await?;
    Ok(Some(user_id))
}

pub async fn auth_token_user(
    db: &DatabaseConnection,
    token: &str,
    kind: AuthTokenKind,
) -> anyhow::Result<Option<Uuid>> {
    Ok(user_auth_token::Entity::find()
        .filter(user_auth_token::Column::TokenHash.eq(grass_token::hash_token(token)))
        .filter(user_auth_token::Column::Kind.eq(kind))
        .filter(user_auth_token::Column::UsedAt.is_null())
        .filter(user_auth_token::Column::ExpiresAt.gt(OffsetDateTime::now_utc()))
        .one(db)
        .await?
        .map(|record| record.user_id))
}

pub async fn verified_mfa_factors(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<user_mfa_factor::Model>> {
    Ok(user_mfa_factor::Entity::find()
        .filter(user_mfa_factor::Column::UserId.eq(user_id))
        .filter(user_mfa_factor::Column::VerifiedAt.is_not_null())
        .order_by_asc(user_mfa_factor::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn mfa_factors(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> anyhow::Result<Vec<user_mfa_factor::Model>> {
    Ok(user_mfa_factor::Entity::find()
        .filter(user_mfa_factor::Column::UserId.eq(user_id))
        .order_by_asc(user_mfa_factor::Column::CreatedAt)
        .all(db)
        .await?)
}

pub async fn mfa_factor(
    db: &DatabaseConnection,
    user_id: Uuid,
    factor_id: Uuid,
) -> anyhow::Result<Option<user_mfa_factor::Model>> {
    Ok(user_mfa_factor::Entity::find_by_id(factor_id)
        .filter(user_mfa_factor::Column::UserId.eq(user_id))
        .one(db)
        .await?)
}

pub async fn start_mfa_factor(
    db: &DatabaseConnection,
    user_id: Uuid,
    kind: MfaFactorKind,
    secret: Option<Vec<u8>>,
    platform_secret: &str,
) -> anyhow::Result<user_mfa_factor::Model> {
    let now = OffsetDateTime::now_utc();
    let existing = user_mfa_factor::Entity::find()
        .filter(user_mfa_factor::Column::UserId.eq(user_id))
        .filter(user_mfa_factor::Column::Kind.eq(kind.clone()))
        .one(db)
        .await?;
    if existing
        .as_ref()
        .is_some_and(|factor| factor.verified_at.is_some())
    {
        anyhow::bail!("MFA factor is already enrolled");
    }
    let factor_id = existing
        .as_ref()
        .map(|factor| factor.id)
        .unwrap_or_else(Uuid::now_v7);
    let secret_envelope = secret
        .as_deref()
        .map(|secret| encrypt_mfa_secret(platform_secret, user_id, factor_id, secret))
        .transpose()?;
    match existing {
        Some(factor) => {
            let mut active: user_mfa_factor::ActiveModel = factor.into();
            active.secret_envelope = Set(secret_envelope);
            active.updated_at = Set(now);
            Ok(active.update(db).await?)
        }
        None => Ok(user_mfa_factor::ActiveModel {
            id: Set(factor_id),
            user_id: Set(user_id),
            kind: Set(kind),
            label: Set(None),
            secret_envelope: Set(secret_envelope),
            verified_at: Set(None),
            last_used_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await?),
    }
}

pub async fn verify_mfa_factor(
    db: &DatabaseConnection,
    factor: user_mfa_factor::Model,
) -> anyhow::Result<user_mfa_factor::Model> {
    let now = OffsetDateTime::now_utc();
    let mut active: user_mfa_factor::ActiveModel = factor.into();
    active.verified_at = Set(Some(now));
    active.last_used_at = Set(Some(now));
    active.updated_at = Set(now);
    Ok(active.update(db).await?)
}

pub async fn mark_mfa_factor_used(
    db: &DatabaseConnection,
    factor: user_mfa_factor::Model,
) -> anyhow::Result<()> {
    let mut active: user_mfa_factor::ActiveModel = factor.into();
    active.last_used_at = Set(Some(OffsetDateTime::now_utc()));
    active.update(db).await?;
    Ok(())
}

pub async fn delete_mfa_factor(
    db: &DatabaseConnection,
    user_id: Uuid,
    factor_id: Uuid,
) -> anyhow::Result<bool> {
    Ok(user_mfa_factor::Entity::delete_many()
        .filter(user_mfa_factor::Column::Id.eq(factor_id))
        .filter(user_mfa_factor::Column::UserId.eq(user_id))
        .exec(db)
        .await?
        .rows_affected
        > 0)
}

pub fn decrypt_mfa_secret(
    platform_secret: &str,
    factor: &user_mfa_factor::Model,
) -> anyhow::Result<Vec<u8>> {
    let envelope: grass_crypto::AeadEnvelope = serde_json::from_value(
        factor
            .secret_envelope
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MFA factor has no secret"))?,
    )?;
    Ok(grass_crypto::decrypt_secret(
        &envelope,
        &authentication_key(platform_secret),
        &mfa_associated_data(factor.user_id, factor.id),
    )?)
}

fn encrypt_mfa_secret(
    platform_secret: &str,
    user_id: Uuid,
    factor_id: Uuid,
    secret: &[u8],
) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::to_value(grass_crypto::encrypt_secret(
        "platform-secret-v1",
        &authentication_key(platform_secret),
        secret,
        &mfa_associated_data(user_id, factor_id),
    )?)?)
}

pub fn authentication_key(platform_secret: &str) -> [u8; 32] {
    Sha256::digest(format!("grass-authentication:v1:{platform_secret}").as_bytes()).into()
}

fn mfa_associated_data(user_id: Uuid, factor_id: Uuid) -> Vec<u8> {
    format!("grass-mfa-factor:v1:{user_id}:{factor_id}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_policy_validates_every_configured_requirement() {
        let policy = PasswordPolicy {
            min_length: 12,
            require_lowercase: true,
            require_uppercase: true,
            require_number: true,
            require_symbol: true,
            ..Default::default()
        };
        assert!(policy.validate_password("Strong-Pass1").is_ok());
        assert!(policy.validate_password("weakpassword").is_err());
        assert!(
            policy
                .validate_password(&policy.generate_password().unwrap())
                .is_ok()
        );
    }

    #[test]
    fn mfa_enforcement_matches_only_the_configured_scope() {
        let user_id = Uuid::now_v7();
        let mut policy = MfaPolicy {
            enforcement: MfaEnforcement::SelectedUsers,
            selected_user_ids: vec![user_id],
            ..Default::default()
        };
        assert!(policy.requires(user_id, &PlatformRole::User));
        assert!(!policy.requires(Uuid::now_v7(), &PlatformRole::Admin));
        policy.enforcement = MfaEnforcement::PlatformAdmins;
        policy.selected_user_ids.clear();
        assert!(policy.requires(Uuid::now_v7(), &PlatformRole::Admin));
    }

    #[test]
    fn mfa_policy_rejects_duplicate_factors_and_selected_users() {
        let mut policy = MfaPolicy {
            allowed_factors: vec!["totp".to_owned(), "totp".to_owned()],
            ..Default::default()
        };
        assert!(policy.validate(false).is_err());

        let user_id = Uuid::now_v7();
        policy.allowed_factors = vec!["totp".to_owned()];
        policy.enforcement = MfaEnforcement::SelectedUsers;
        policy.selected_user_ids = vec![user_id, user_id];
        assert!(policy.validate(false).is_err());
    }

    #[test]
    fn generated_password_respects_a_short_strict_policy() {
        let policy = PasswordPolicy {
            min_length: 8,
            max_length: 8,
            require_lowercase: true,
            require_uppercase: true,
            require_number: true,
            require_symbol: true,
            history_count: 20,
        };
        let generated = policy.generate_password().unwrap();
        assert_eq!(generated.len(), 8);
        assert!(policy.validate_password(&generated).is_ok());
    }
}
