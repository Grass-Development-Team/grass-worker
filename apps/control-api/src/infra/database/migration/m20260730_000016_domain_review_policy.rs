use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE host_review_status AS ENUM ('not_required', 'pending', 'approved', 'rejected');

ALTER TABLE team_groups
    DROP CONSTRAINT ck_team_groups_review_policy,
    ADD CONSTRAINT ck_team_groups_review_policy
        CHECK (
            review_policy IS NULL
            OR (
                jsonb_typeof(review_policy) = 'object'
                AND review_policy - 'production' - 'preview' - 'domain' = '{}'::jsonb
                AND (
                    NOT review_policy ? 'production'
                    OR review_policy ->> 'production' IN ('auto', 'manual')
                )
                AND (
                    NOT review_policy ? 'preview'
                    OR review_policy ->> 'preview' IN ('auto', 'manual')
                )
                AND (
                    NOT review_policy ? 'domain'
                    OR review_policy ->> 'domain' IN ('auto', 'manual')
                )
            )
        );

ALTER TABLE project_host_bindings
    ADD COLUMN review_status host_review_status NULL,
    ADD COLUMN reviewed_by_user_id UUID NULL,
    ADD COLUMN reviewed_at TIMESTAMPTZ NULL,
    ADD COLUMN review_reason TEXT NULL;

UPDATE project_host_bindings
SET review_status = CASE
    WHEN kind = 'custom' THEN 'approved'::host_review_status
    ELSE 'not_required'::host_review_status
END;

ALTER TABLE project_host_bindings
    ALTER COLUMN review_status SET NOT NULL,
    ADD CONSTRAINT fk_project_host_bindings_reviewed_by_user_id
        FOREIGN KEY (reviewed_by_user_id) REFERENCES users (id) ON DELETE SET NULL;

CREATE INDEX ix_project_host_bindings_pending_review
    ON project_host_bindings (created_at)
    WHERE review_status = 'pending' AND deleted_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_project_host_bindings_pending_review;

ALTER TABLE project_host_bindings
    DROP CONSTRAINT fk_project_host_bindings_reviewed_by_user_id,
    DROP COLUMN review_status,
    DROP COLUMN reviewed_by_user_id,
    DROP COLUMN reviewed_at,
    DROP COLUMN review_reason;

DROP TYPE host_review_status;

ALTER TABLE team_groups
    DROP CONSTRAINT ck_team_groups_review_policy;

UPDATE team_groups
SET review_policy = review_policy - 'domain'
WHERE review_policy ? 'domain';

ALTER TABLE team_groups
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
    fn migration_adds_review_state_backfill_index_and_domain_override() {
        assert!(UP_SQL.contains(
            "CREATE TYPE host_review_status AS ENUM ('not_required', 'pending', 'approved', 'rejected')"
        ));
        assert!(UP_SQL.contains("ADD COLUMN review_status host_review_status NULL"));
        assert!(UP_SQL.contains("ADD COLUMN reviewed_by_user_id UUID NULL"));
        assert!(UP_SQL.contains("ADD COLUMN reviewed_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("ADD COLUMN review_reason TEXT NULL"));
        assert!(!UP_SQL.contains("reviewed_at TIMESTAMPTZ NULL DEFAULT"));
        assert!(!UP_SQL.contains("review_reason TEXT NULL DEFAULT"));
        assert!(UP_SQL.contains("WHEN kind = 'custom' THEN 'approved'::host_review_status"));
        assert!(UP_SQL.contains("ELSE 'not_required'::host_review_status"));
        assert!(UP_SQL.contains("ALTER COLUMN review_status SET NOT NULL"));
        assert!(UP_SQL.contains("fk_project_host_bindings_reviewed_by_user_id"));
        assert!(UP_SQL.contains("REFERENCES users (id) ON DELETE SET NULL"));
        assert!(UP_SQL.contains("ix_project_host_bindings_pending_review"));
        assert!(UP_SQL.contains("WHERE review_status = 'pending'"));
        assert!(UP_SQL.contains("review_policy - 'production' - 'preview' - 'domain'"));
        assert!(UP_SQL.contains("review_policy ->> 'domain' IN ('auto', 'manual')"));
    }

    #[test]
    fn migration_is_reversible_and_restores_the_previous_team_group_constraint() {
        assert!(DOWN_SQL.contains("review_policy = review_policy - 'domain'"));
        assert!(DOWN_SQL.contains("review_policy - 'production' - 'preview' = '{}'::jsonb"));
        assert!(DOWN_SQL.contains("DROP COLUMN review_status"));
        assert!(DOWN_SQL.contains("DROP COLUMN reviewed_by_user_id"));
        assert!(DOWN_SQL.contains("DROP COLUMN reviewed_at"));
        assert!(DOWN_SQL.contains("DROP COLUMN review_reason"));
        assert!(DOWN_SQL.contains("DROP TYPE host_review_status"));
    }
}
