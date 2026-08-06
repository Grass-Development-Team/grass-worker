use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE identity_provider_kind AS ENUM ('oidc', 'github');
CREATE TYPE auth_token_kind AS ENUM ('email_verification', 'password_reset');
CREATE TYPE mfa_factor_kind AS ENUM ('totp', 'email');

ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ NULL;
UPDATE users SET email_verified_at = created_at WHERE email_verified_at IS NULL;

CREATE TABLE auth_identity_providers (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    kind identity_provider_kind NOT NULL,
    name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    client_id TEXT NOT NULL,
    client_secret_envelope JSONB NOT NULL,
    issuer_url TEXT NULL,
    authorization_url TEXT NOT NULL,
    token_url TEXT NOT NULL,
    userinfo_url TEXT NULL,
    jwks_url TEXT NULL,
    scopes JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_auth_identity_providers_slug
        CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,62}$'),
    CONSTRAINT ck_auth_identity_providers_name
        CHECK (btrim(name) <> '' AND char_length(name) <= 120),
    CONSTRAINT ck_auth_identity_providers_client_id CHECK (btrim(client_id) <> ''),
    CONSTRAINT ck_auth_identity_providers_oidc
        CHECK (kind <> 'oidc' OR (issuer_url IS NOT NULL AND jwks_url IS NOT NULL))
);

CREATE TABLE user_external_identities (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider_id UUID NOT NULL REFERENCES auth_identity_providers(id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    email TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_user_external_identities_provider_subject UNIQUE (provider_id, subject),
    CONSTRAINT uq_user_external_identities_user_provider UNIQUE (user_id, provider_id),
    CONSTRAINT ck_user_external_identities_subject CHECK (btrim(subject) <> '')
);

CREATE INDEX ix_user_external_identities_user_id
    ON user_external_identities (user_id);

CREATE TABLE user_auth_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind auth_token_kind NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX ix_user_auth_tokens_live
    ON user_auth_tokens (user_id, kind, expires_at)
    WHERE used_at IS NULL;

CREATE TABLE user_mfa_factors (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind mfa_factor_kind NOT NULL,
    label TEXT NULL,
    secret_envelope JSONB NULL,
    verified_at TIMESTAMPTZ NULL,
    last_used_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_user_mfa_factors_user_kind UNIQUE (user_id, kind),
    CONSTRAINT ck_user_mfa_factors_secret
        CHECK ((kind = 'totp' AND secret_envelope IS NOT NULL) OR
               (kind = 'email' AND secret_envelope IS NULL))
);

CREATE INDEX ix_user_mfa_factors_verified
    ON user_mfa_factors (user_id, kind)
    WHERE verified_at IS NOT NULL;

CREATE TABLE user_password_history (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX ix_user_password_history_recent
    ON user_password_history (user_id, created_at DESC, id DESC);

INSERT INTO user_password_history (id, user_id, password_hash, created_at)
SELECT gen_random_uuid(), user_id, password_hash, created_at
FROM user_password_credentials;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE user_password_history;
DROP TABLE user_mfa_factors;
DROP TABLE user_auth_tokens;
DROP TABLE user_external_identities;
DROP TABLE auth_identity_providers;
ALTER TABLE users DROP COLUMN email_verified_at;
DROP TYPE mfa_factor_kind;
DROP TYPE auth_token_kind;
DROP TYPE identity_provider_kind;
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_backfills_email_verification_and_creates_auth_state() {
        assert!(UP_SQL.contains("UPDATE users SET email_verified_at = created_at"));
        assert!(UP_SQL.contains("CREATE TABLE auth_identity_providers"));
        assert!(UP_SQL.contains("CREATE TABLE user_external_identities"));
        assert!(UP_SQL.contains("CREATE TABLE user_auth_tokens"));
        assert!(UP_SQL.contains("CREATE TABLE user_mfa_factors"));
        assert!(UP_SQL.contains("CREATE TABLE user_password_history"));
        assert!(UP_SQL.contains("WHERE verified_at IS NOT NULL"));
    }

    #[test]
    fn migration_is_reversible() {
        assert!(DOWN_SQL.contains("ALTER TABLE users DROP COLUMN email_verified_at"));
        assert!(DOWN_SQL.contains("DROP TYPE identity_provider_kind"));
    }
}
