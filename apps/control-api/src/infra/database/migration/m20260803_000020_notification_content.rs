use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE user_notifications
    ADD COLUMN title TEXT NULL,
    ADD COLUMN content TEXT NULL,
    ALTER COLUMN project_name DROP NOT NULL,
    ALTER COLUMN project_slug DROP NOT NULL;

ALTER TABLE user_notifications
    ADD CONSTRAINT ck_user_notifications_announcement_content
        CHECK (
            action <> 'site.announcement'
            OR (
                title IS NOT NULL
                AND btrim(title) <> ''
                AND content IS NOT NULL
                AND btrim(content) <> ''
                AND team_id IS NULL
                AND project_id IS NULL
            )
        );
"#;

pub(crate) const DOWN_SQL: &str = r#"
DELETE FROM user_notifications
WHERE action = 'site.announcement';

ALTER TABLE user_notifications
    DROP CONSTRAINT ck_user_notifications_announcement_content,
    DROP COLUMN content,
    DROP COLUMN title,
    ALTER COLUMN project_name SET NOT NULL,
    ALTER COLUMN project_slug SET NOT NULL;
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
    fn migration_adds_optional_content_fields_and_announcement_constraint() {
        assert!(UP_SQL.contains("ADD COLUMN title TEXT NULL"));
        assert!(UP_SQL.contains("ADD COLUMN content TEXT NULL"));
        assert!(UP_SQL.contains("ALTER COLUMN project_name DROP NOT NULL"));
        assert!(UP_SQL.contains("ALTER COLUMN project_slug DROP NOT NULL"));
        assert!(UP_SQL.contains("ck_user_notifications_announcement_content"));
        assert!(UP_SQL.contains("action <> 'site.announcement'"));
        assert!(UP_SQL.contains("team_id IS NULL"));
        assert!(UP_SQL.contains("project_id IS NULL"));
    }

    #[test]
    fn migration_removes_announcement_rows_before_restoring_project_constraints() {
        assert!(DOWN_SQL.contains("DELETE FROM user_notifications"));
        assert!(DOWN_SQL.contains("action = 'site.announcement'"));
        assert!(DOWN_SQL.contains("DROP COLUMN content"));
        assert!(DOWN_SQL.contains("DROP COLUMN title"));
        assert!(DOWN_SQL.contains("ALTER COLUMN project_name SET NOT NULL"));
        assert!(DOWN_SQL.contains("ALTER COLUMN project_slug SET NOT NULL"));
    }
}
