use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP_SQL: &str = r#"
ALTER TYPE deployment_serve_status ADD VALUE IF NOT EXISTS 'retired';

ALTER TABLE deployments
    ADD COLUMN pending_release_reason release_reason NULL,
    ADD COLUMN pending_release_actor_user_id UUID NULL,
    ADD COLUMN pending_release_requested_at TIMESTAMPTZ NULL,
    ADD CONSTRAINT fk_deployments_pending_release_actor_user_id
        FOREIGN KEY (pending_release_actor_user_id) REFERENCES users(id) ON DELETE SET NULL,
    ADD CONSTRAINT ck_deployments_pending_release_reason
        CHECK (pending_release_reason IS NULL OR pending_release_reason <> 'auto'),
    ADD CONSTRAINT ck_deployments_pending_release_requested_at
        CHECK (pending_release_reason IS NULL OR pending_release_requested_at IS NOT NULL);

CREATE UNIQUE INDEX ux_deployments_one_pending_release_per_environment
    ON deployments (project_id, environment)
    WHERE pending_release_reason IS NOT NULL AND deleted_at IS NULL;
"#;

const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ux_deployments_one_pending_release_per_environment;

ALTER TABLE deployments
    DROP CONSTRAINT fk_deployments_pending_release_actor_user_id,
    DROP CONSTRAINT ck_deployments_pending_release_reason,
    DROP CONSTRAINT ck_deployments_pending_release_requested_at,
    DROP COLUMN pending_release_requested_at,
    DROP COLUMN pending_release_actor_user_id,
    DROP COLUMN pending_release_reason;

UPDATE deployments SET serve_status = 'pending' WHERE serve_status = 'retired';
ALTER TABLE deployments ALTER COLUMN serve_status DROP DEFAULT;
ALTER TYPE deployment_serve_status RENAME TO deployment_serve_status_old;
CREATE TYPE deployment_serve_status AS ENUM ('pending', 'syncing', 'ready', 'failed');
ALTER TABLE deployments
    ALTER COLUMN serve_status TYPE deployment_serve_status
    USING serve_status::text::deployment_serve_status;
ALTER TABLE deployments ALTER COLUMN serve_status SET DEFAULT 'pending';
DROP TYPE deployment_serve_status_old;
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
    fn migration_adds_reversible_delivery_rollout_state() {
        assert!(UP_SQL.contains("ADD VALUE IF NOT EXISTS 'retired'"));
        assert!(UP_SQL.contains("pending_release_reason release_reason NULL"));
        assert!(UP_SQL.contains("pending_release_actor_user_id UUID NULL"));
        assert!(UP_SQL.contains("pending_release_requested_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("WHERE pending_release_reason IS NOT NULL"));
        assert!(DOWN_SQL.contains("serve_status = 'pending'"));
        assert!(DOWN_SQL.contains("DROP TYPE deployment_serve_status_old"));
    }
}
