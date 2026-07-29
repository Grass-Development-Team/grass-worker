use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE team_groups
    ADD COLUMN review_policy JSONB NULL,
    ADD CONSTRAINT ck_team_groups_review_policy
        CHECK (
            review_policy IS NULL
            OR (
                jsonb_typeof(review_policy) = 'object'
                AND review_policy - 'production' - 'preview' = '{}'::jsonb
                AND (
                    NOT review_policy ? 'production'
                    OR review_policy ->> 'production' IN ('auto', 'manual')
                )
                AND (
                    NOT review_policy ? 'preview'
                    OR review_policy ->> 'preview' IN ('auto', 'manual')
                )
            )
        );
"#;

pub(crate) const DOWN_SQL: &str = r#"
ALTER TABLE team_groups
    DROP CONSTRAINT ck_team_groups_review_policy,
    DROP COLUMN review_policy;
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
    fn migration_adds_a_validated_reversible_review_policy_override() {
        assert!(UP_SQL.contains("ADD COLUMN review_policy JSONB NULL"));
        assert!(UP_SQL.contains("ck_team_groups_review_policy"));
        assert!(UP_SQL.contains("review_policy ->> 'production' IN ('auto', 'manual')"));
        assert!(UP_SQL.contains("review_policy ->> 'preview' IN ('auto', 'manual')"));
        assert!(DOWN_SQL.contains("DROP COLUMN review_policy"));
    }
}
