use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE codes (
    id UUID PRIMARY KEY,
    scope TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    token_suffix TEXT NOT NULL,
    expires_at TIMESTAMPTZ NULL,
    used_at TIMESTAMPTZ NULL,
    used_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    revoked_at TIMESTAMPTZ NULL,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_codes_scope CHECK (btrim(scope) <> '' AND char_length(scope) <= 64),
    CONSTRAINT ck_codes_token_prefix CHECK (char_length(token_prefix) = 6),
    CONSTRAINT ck_codes_token_suffix CHECK (char_length(token_suffix) = 4),
    CONSTRAINT ck_codes_usage CHECK (used_by_user_id IS NULL OR used_at IS NOT NULL)
);

CREATE INDEX ix_codes_scope_created
    ON codes (scope, created_at DESC, id DESC);
CREATE INDEX ix_codes_available
    ON codes (scope, expires_at)
    WHERE used_at IS NULL AND revoked_at IS NULL;
CREATE INDEX ix_codes_used_by_user
    ON codes (used_by_user_id)
    WHERE used_by_user_id IS NOT NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE codes;
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
    fn creates_hashed_scoped_codes_with_lifecycle_tracking() {
        assert!(UP_SQL.contains("CREATE TABLE codes"));
        assert!(UP_SQL.contains("scope TEXT NOT NULL"));
        assert!(UP_SQL.contains("token_hash TEXT NOT NULL UNIQUE"));
        assert!(UP_SQL.contains("token_prefix TEXT NOT NULL"));
        assert!(UP_SQL.contains("token_suffix TEXT NOT NULL"));
        assert!(UP_SQL.contains("expires_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("used_at TIMESTAMPTZ NULL"));
        assert!(
            UP_SQL.contains("used_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL")
        );
        assert!(UP_SQL.contains("revoked_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("WHERE used_at IS NULL AND revoked_at IS NULL"));
    }

    #[test]
    fn scoped_codes_migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE codes"));
    }
}
