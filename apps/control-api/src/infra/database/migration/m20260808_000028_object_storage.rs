use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE storage_migration_status AS ENUM (
    'pending',
    'running',
    'succeeded',
    'failed'
);

CREATE TYPE storage_migration_object_status AS ENUM (
    'pending',
    'running',
    'succeeded',
    'failed'
);

CREATE TABLE storage_migration_jobs (
    id UUID PRIMARY KEY,
    status storage_migration_status NOT NULL DEFAULT 'pending',
    source_config JSONB NOT NULL,
    source_credentials JSONB NULL,
    target_config JSONB NOT NULL,
    target_credentials JSONB NULL,
    copied_objects BIGINT NOT NULL DEFAULT 0,
    copied_bytes BIGINT NOT NULL DEFAULT 0,
    total_objects BIGINT NULL,
    total_bytes BIGINT NULL,
    last_error TEXT NULL,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL,
    started_at TIMESTAMPTZ NULL,
    finished_at TIMESTAMPTZ NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_storage_migration_jobs_counts
        CHECK (copied_objects >= 0 AND copied_bytes >= 0
            AND (total_objects IS NULL OR total_objects >= 0)
            AND (total_bytes IS NULL OR total_bytes >= 0)),
    CONSTRAINT ck_storage_migration_jobs_error
        CHECK (last_error IS NULL OR char_length(last_error) <= 1000)
);

CREATE UNIQUE INDEX ux_storage_migration_jobs_active
    ON storage_migration_jobs ((1))
    WHERE status IN ('pending', 'running');

CREATE TABLE storage_migration_objects (
    job_id UUID NOT NULL REFERENCES storage_migration_jobs(id) ON DELETE CASCADE,
    object_key TEXT NOT NULL,
    source_size BIGINT NOT NULL,
    status storage_migration_object_status NOT NULL DEFAULT 'pending',
    checksum_sha256 TEXT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (job_id, object_key),
    CONSTRAINT ck_storage_migration_objects_key
        CHECK (char_length(object_key) > 0 AND char_length(object_key) <= 1024),
    CONSTRAINT ck_storage_migration_objects_counts
        CHECK (source_size >= 0 AND attempt_count >= 0),
    CONSTRAINT ck_storage_migration_objects_error
        CHECK (last_error IS NULL OR char_length(last_error) <= 1000)
);

CREATE INDEX ix_storage_migration_objects_due
    ON storage_migration_objects (job_id, status, updated_at);
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE storage_migration_objects;
DROP INDEX ux_storage_migration_jobs_active;
DROP TABLE storage_migration_jobs;
DROP TYPE storage_migration_object_status;
DROP TYPE storage_migration_status;
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
    fn creates_resumable_storage_migration_tables() {
        assert!(UP_SQL.contains("CREATE TABLE storage_migration_jobs"));
        assert!(UP_SQL.contains("CREATE TABLE storage_migration_objects"));
        assert!(UP_SQL.contains("ON storage_migration_jobs ((1))"));
        assert!(UP_SQL.contains("WHERE status IN ('pending', 'running')"));
        assert!(UP_SQL.contains("PRIMARY KEY (job_id, object_key)"));
    }

    #[test]
    fn migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE storage_migration_objects"));
        assert!(DOWN_SQL.contains("DROP TYPE storage_migration_status"));
    }
}
