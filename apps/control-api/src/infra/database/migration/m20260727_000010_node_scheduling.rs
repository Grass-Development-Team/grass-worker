use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP_SQL: &str = r#"
CREATE TYPE deployment_serve_status AS ENUM ('pending', 'syncing', 'ready', 'failed');
ALTER TYPE deployment_event_kind ADD VALUE IF NOT EXISTS 'serve';

ALTER TABLE nodes
    ADD COLUMN capacity_cpu_millicores BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN capacity_memory_mb BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN capacity_disk_mb BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN max_deployments INTEGER NOT NULL DEFAULT 10,
    ADD CONSTRAINT ck_nodes_capacity_cpu_millicores_nonnegative
        CHECK (capacity_cpu_millicores >= 0),
    ADD CONSTRAINT ck_nodes_capacity_memory_mb_nonnegative
        CHECK (capacity_memory_mb >= 0),
    ADD CONSTRAINT ck_nodes_capacity_disk_mb_nonnegative
        CHECK (capacity_disk_mb >= 0),
    ADD CONSTRAINT ck_nodes_max_deployments_positive
        CHECK (max_deployments > 0);

ALTER TABLE deployments RENAME COLUMN node_id TO build_node_id;
ALTER TABLE deployments
    RENAME CONSTRAINT fk_deployments_node_id TO fk_deployments_build_node_id;
ALTER TABLE deployments
    ADD COLUMN serve_node_id UUID NULL,
    ADD COLUMN serve_status deployment_serve_status NOT NULL DEFAULT 'pending',
    ADD COLUMN serve_cpu_millicores BIGINT NOT NULL DEFAULT 50,
    ADD COLUMN serve_memory_mb BIGINT NOT NULL DEFAULT 64,
    ADD COLUMN serve_disk_mb BIGINT NOT NULL DEFAULT 256,
    ADD COLUMN overcommitted BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN serve_failure_code TEXT NULL,
    ADD COLUMN serve_failure_message TEXT NULL,
    ADD COLUMN serve_started_at TIMESTAMPTZ NULL,
    ADD COLUMN serve_finished_at TIMESTAMPTZ NULL,
    ADD CONSTRAINT fk_deployments_serve_node_id
        FOREIGN KEY (serve_node_id) REFERENCES nodes(id) ON DELETE SET NULL,
    ADD CONSTRAINT ck_deployments_serve_cpu_millicores_positive
        CHECK (serve_cpu_millicores > 0),
    ADD CONSTRAINT ck_deployments_serve_memory_mb_positive
        CHECK (serve_memory_mb > 0),
    ADD CONSTRAINT ck_deployments_serve_disk_mb_positive
        CHECK (serve_disk_mb > 0);

UPDATE deployments
SET
    serve_node_id = COALESCE(
        build_node_id,
        (
            SELECT id
            FROM nodes
            WHERE deleted_at IS NULL AND serve_enabled
            ORDER BY created_at, id
            LIMIT 1
        )
    ),
    serve_cpu_millicores = CASE runtime_kind WHEN 'ssr' THEN 200 ELSE 50 END,
    serve_memory_mb = CASE runtime_kind WHEN 'ssr' THEN 256 ELSE 64 END,
    serve_disk_mb = CASE runtime_kind WHEN 'ssr' THEN 512 ELSE 256 END;

CREATE INDEX ix_deployments_serve_node_status
    ON deployments (serve_node_id, serve_status)
    WHERE deleted_at IS NULL;

WITH ranked AS (
    SELECT
        id,
        row_number() OVER (
            PARTITION BY deployment_id, kind
            ORDER BY created_at DESC, id DESC
        ) AS position
    FROM deployment_artifacts
)
DELETE FROM deployment_artifacts
WHERE id IN (SELECT id FROM ranked WHERE position > 1);

CREATE UNIQUE INDEX uq_deployment_artifacts_deployment_kind
    ON deployment_artifacts (deployment_id, kind);
"#;

const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS uq_deployment_artifacts_deployment_kind;
DROP INDEX IF EXISTS ix_deployments_serve_node_status;

ALTER TABLE deployments
    DROP COLUMN serve_finished_at,
    DROP COLUMN serve_started_at,
    DROP COLUMN serve_failure_message,
    DROP COLUMN serve_failure_code,
    DROP COLUMN overcommitted,
    DROP COLUMN serve_disk_mb,
    DROP COLUMN serve_memory_mb,
    DROP COLUMN serve_cpu_millicores,
    DROP COLUMN serve_status,
    DROP COLUMN serve_node_id;
ALTER TABLE deployments
    RENAME CONSTRAINT fk_deployments_build_node_id TO fk_deployments_node_id;
ALTER TABLE deployments RENAME COLUMN build_node_id TO node_id;

ALTER TABLE nodes
    DROP COLUMN max_deployments,
    DROP COLUMN capacity_disk_mb,
    DROP COLUMN capacity_memory_mb,
    DROP COLUMN capacity_cpu_millicores;

UPDATE deployment_events SET kind = 'system' WHERE kind = 'serve';
ALTER TABLE deployment_events ALTER COLUMN kind DROP DEFAULT;
ALTER TYPE deployment_event_kind RENAME TO deployment_event_kind_old;
CREATE TYPE deployment_event_kind AS ENUM ('system', 'build', 'release', 'review', 'host');
ALTER TABLE deployment_events
    ALTER COLUMN kind TYPE deployment_event_kind
    USING kind::text::deployment_event_kind;
ALTER TABLE deployment_events ALTER COLUMN kind SET DEFAULT 'system';
DROP TYPE deployment_event_kind_old;
DROP TYPE deployment_serve_status;
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The metadata rows removed by the up migration cannot be recreated.
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
    fn migration_contains_runtime_defaults_and_reversible_event_enum() {
        assert!(UP_SQL.contains("WHEN 'ssr' THEN 200 ELSE 50"));
        assert!(UP_SQL.contains("WHEN 'ssr' THEN 256 ELSE 64"));
        assert!(UP_SQL.contains("WHEN 'ssr' THEN 512 ELSE 256"));
        assert!(DOWN_SQL.contains("SET kind = 'system' WHERE kind = 'serve'"));
        assert!(DOWN_SQL.contains("DROP TYPE deployment_serve_status"));
    }
}
