use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TYPE deployment_artifact_kind ADD VALUE IF NOT EXISTS 'screenshot';

CREATE TYPE deployment_screenshot_status AS ENUM (
    'pending',
    'running',
    'succeeded',
    'failed'
);

CREATE TABLE deployment_screenshot_jobs (
    deployment_id UUID PRIMARY KEY REFERENCES deployments(id) ON DELETE CASCADE,
    status deployment_screenshot_status NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    last_error TEXT NULL,
    artifact_id UUID NULL REFERENCES deployment_artifacts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_deployment_screenshot_attempt_count
        CHECK (attempt_count >= 0 AND attempt_count <= 4),
    CONSTRAINT ck_deployment_screenshot_last_error
        CHECK (last_error IS NULL OR char_length(last_error) <= 500),
    CONSTRAINT ck_deployment_screenshot_artifact
        CHECK ((status = 'succeeded') = (artifact_id IS NOT NULL))
);

CREATE INDEX ix_deployment_screenshot_jobs_due
    ON deployment_screenshot_jobs (next_attempt_at, deployment_id)
    WHERE status = 'pending';
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE deployment_screenshot_jobs;

DELETE FROM deployment_artifacts WHERE kind = 'screenshot';
ALTER TYPE deployment_artifact_kind RENAME TO deployment_artifact_kind_with_screenshot;
CREATE TYPE deployment_artifact_kind AS ENUM ('grass_output', 'build_log', 'static_site');
ALTER TABLE deployment_artifacts
    ALTER COLUMN kind TYPE deployment_artifact_kind
    USING kind::text::deployment_artifact_kind;
DROP TYPE deployment_artifact_kind_with_screenshot;
DROP TYPE deployment_screenshot_status;
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
    fn adds_screenshot_artifacts_and_bounded_retry_jobs() {
        assert!(UP_SQL.contains("ADD VALUE IF NOT EXISTS 'screenshot'"));
        assert!(UP_SQL.contains("CREATE TABLE deployment_screenshot_jobs"));
        assert!(UP_SQL.contains("attempt_count >= 0 AND attempt_count <= 4"));
        assert!(UP_SQL.contains("WHERE status = 'pending'"));
        assert!(UP_SQL.contains(
            "artifact_id UUID NULL REFERENCES deployment_artifacts(id) ON DELETE CASCADE"
        ));
    }

    #[test]
    fn deployment_screenshot_migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE deployment_screenshot_jobs"));
        assert!(DOWN_SQL.contains("DELETE FROM deployment_artifacts WHERE kind = 'screenshot'"));
        assert!(DOWN_SQL.contains("DROP TYPE deployment_screenshot_status"));
    }
}
