use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE node_deletion_status AS ENUM (
    'queued', 'migrating', 'draining', 'deleting', 'failed', 'completed'
);
CREATE TYPE node_deployment_migration_status AS ENUM (
    'pending', 'syncing', 'ready', 'failed'
);

CREATE TABLE node_deletion_jobs (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    target_node_id UUID NULL REFERENCES nodes(id) ON DELETE RESTRICT,
    requested_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    status node_deletion_status NOT NULL DEFAULT 'queued',
    total_deployments INTEGER NOT NULL DEFAULT 0,
    migrated_deployments INTEGER NOT NULL DEFAULT 0,
    active_builds INTEGER NOT NULL DEFAULT 0,
    error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ NULL,
    CONSTRAINT ck_node_deletion_jobs_distinct_target
        CHECK (target_node_id IS NULL OR target_node_id <> node_id),
    CONSTRAINT ck_node_deletion_jobs_progress_nonnegative
        CHECK (
            total_deployments >= 0
            AND migrated_deployments >= 0
            AND active_builds >= 0
        ),
    CONSTRAINT ck_node_deletion_jobs_progress_bounded
        CHECK (migrated_deployments <= total_deployments),
    CONSTRAINT ck_node_deletion_jobs_completed_at
        CHECK ((status = 'completed') = (completed_at IS NOT NULL))
);

CREATE UNIQUE INDEX ux_node_deletion_jobs_active_node
    ON node_deletion_jobs (node_id)
    WHERE status <> 'completed';
CREATE INDEX ix_node_deletion_jobs_queue
    ON node_deletion_jobs (status, updated_at)
    WHERE status NOT IN ('failed', 'completed');

CREATE TABLE node_deployment_migrations (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES node_deletion_jobs(id) ON DELETE CASCADE,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE RESTRICT,
    target_node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE RESTRICT,
    status node_deployment_migration_status NOT NULL DEFAULT 'pending',
    error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    ready_at TIMESTAMPTZ NULL,
    CONSTRAINT ux_node_deployment_migrations_job_deployment
        UNIQUE (job_id, deployment_id),
    CONSTRAINT ck_node_deployment_migrations_distinct_nodes
        CHECK (source_node_id <> target_node_id),
    CONSTRAINT ck_node_deployment_migrations_ready_at
        CHECK ((status = 'ready') = (ready_at IS NOT NULL))
);

CREATE INDEX ix_node_deployment_migrations_target
    ON node_deployment_migrations (target_node_id, status, created_at)
    WHERE status IN ('pending', 'syncing', 'ready');
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX IF EXISTS ix_node_deployment_migrations_target;
DROP TABLE IF EXISTS node_deployment_migrations;
DROP INDEX IF EXISTS ix_node_deletion_jobs_queue;
DROP INDEX IF EXISTS ux_node_deletion_jobs_active_node;
DROP TABLE IF EXISTS node_deletion_jobs;
DROP TYPE node_deployment_migration_status;
DROP TYPE node_deletion_status;
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
    fn migration_adds_reversible_node_deletion_and_shadow_migration_queues() {
        assert!(UP_SQL.contains("CREATE TYPE node_deletion_status"));
        assert!(UP_SQL.contains("CREATE TYPE node_deployment_migration_status"));
        assert!(UP_SQL.contains("CREATE TABLE node_deletion_jobs"));
        assert!(UP_SQL.contains("CREATE TABLE node_deployment_migrations"));
        assert!(UP_SQL.contains("ux_node_deletion_jobs_active_node"));
        assert!(UP_SQL.contains("ck_node_deletion_jobs_completed_at"));
        assert!(UP_SQL.contains("ck_node_deployment_migrations_ready_at"));
        assert!(UP_SQL.contains("WHERE status IN ('pending', 'syncing', 'ready')"));
        assert!(DOWN_SQL.contains("DROP TYPE node_deletion_status"));
    }
}
