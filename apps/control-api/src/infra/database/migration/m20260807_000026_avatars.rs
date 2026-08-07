use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE users ADD COLUMN avatar_version UUID NULL;
ALTER TABLE teams ADD COLUMN avatar_version UUID NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
ALTER TABLE teams DROP COLUMN avatar_version;
ALTER TABLE users DROP COLUMN avatar_version;
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
    fn adds_nullable_avatar_versions_without_defaults() {
        assert!(UP_SQL.contains("users ADD COLUMN avatar_version UUID NULL"));
        assert!(UP_SQL.contains("teams ADD COLUMN avatar_version UUID NULL"));
        assert!(!UP_SQL.contains("DEFAULT"));
    }

    #[test]
    fn avatar_version_migration_is_reversible() {
        assert!(DOWN_SQL.contains("teams DROP COLUMN avatar_version"));
        assert!(DOWN_SQL.contains("users DROP COLUMN avatar_version"));
    }
}
