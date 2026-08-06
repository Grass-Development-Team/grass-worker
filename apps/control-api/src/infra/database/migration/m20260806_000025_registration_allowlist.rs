use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE registration_email_allowlist (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_registration_email_allowlist_email
        CHECK (btrim(email) <> '' AND char_length(email) <= 320 AND email = lower(email))
);

CREATE INDEX ix_registration_email_allowlist_created
    ON registration_email_allowlist (created_at DESC, id DESC);
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE registration_email_allowlist;
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
    fn creates_a_normalized_registration_email_allowlist() {
        assert!(UP_SQL.contains("CREATE TABLE registration_email_allowlist"));
        assert!(UP_SQL.contains("email TEXT NOT NULL UNIQUE"));
        assert!(
            UP_SQL.contains("created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL")
        );
        assert!(UP_SQL.contains("email = lower(email)"));
    }

    #[test]
    fn registration_allowlist_migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE registration_email_allowlist"));
    }
}
