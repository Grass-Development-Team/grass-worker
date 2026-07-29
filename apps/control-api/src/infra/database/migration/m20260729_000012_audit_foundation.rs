use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TYPE audit_actor_type AS ENUM ('anonymous', 'user', 'system', 'node');
CREATE TYPE audit_event_visibility AS ENUM ('platform', 'team');

ALTER TABLE audit_events
    ADD COLUMN actor_type audit_actor_type NOT NULL DEFAULT 'system',
    ADD COLUMN actor_node_id UUID NULL,
    ADD COLUMN visibility audit_event_visibility NOT NULL DEFAULT 'platform',
    ADD COLUMN request_id UUID NULL,
    ADD COLUMN source_ip TEXT NULL,
    ADD COLUMN user_agent TEXT NULL,
    ADD COLUMN http_method TEXT NULL,
    ADD COLUMN request_path TEXT NULL,
    ADD COLUMN status_code INTEGER NULL,
    ADD COLUMN duration_ms BIGINT NULL,
    ADD COLUMN changes JSONB NOT NULL DEFAULT '{}',
    ADD CONSTRAINT fk_audit_events_actor_node_id
        FOREIGN KEY (actor_node_id) REFERENCES nodes(id) ON DELETE SET NULL,
    ADD CONSTRAINT ck_audit_events_status_code
        CHECK (status_code IS NULL OR status_code BETWEEN 100 AND 599),
    ADD CONSTRAINT ck_audit_events_duration_ms
        CHECK (duration_ms IS NULL OR duration_ms >= 0);

UPDATE audit_events
SET
    actor_type = CASE
        WHEN actor_user_id IS NOT NULL THEN 'user'::audit_actor_type
        ELSE 'system'::audit_actor_type
    END,
    visibility = CASE
        WHEN team_id IS NOT NULL
            AND COALESCE(metadata ->> 'platform_admin', 'false') <> 'true'
            AND COALESCE(metadata ->> 'completed_after_sync', 'false') <> 'true'
            AND action NOT IN (
                'team.created',
                'team.updated',
                'team.deleted',
                'team.quota_plan_overridden',
                'team.group_changed'
            )
            THEN 'team'::audit_event_visibility
        ELSE 'platform'::audit_event_visibility
    END;

ALTER TABLE audit_events
    ADD CONSTRAINT ck_audit_events_actor_identity
        CHECK (
            (actor_user_id IS NULL OR actor_type = 'user')
            AND (actor_node_id IS NULL OR actor_type = 'node')
            AND (
                actor_type NOT IN ('anonymous', 'system')
                OR (actor_user_id IS NULL AND actor_node_id IS NULL)
            )
        );

ALTER TABLE deployments
    ADD COLUMN pending_release_audit_visibility audit_event_visibility NULL;

UPDATE deployments
SET pending_release_audit_visibility = 'platform'
WHERE pending_release_reason IS NOT NULL;

ALTER TABLE deployments
    ADD CONSTRAINT ck_deployments_pending_release_audit_visibility
        CHECK (
            (pending_release_reason IS NULL)
            = (pending_release_audit_visibility IS NULL)
        );

CREATE UNIQUE INDEX ux_audit_events_request_id
    ON audit_events (request_id) WHERE request_id IS NOT NULL;
CREATE INDEX ix_audit_events_visibility_created_at
    ON audit_events (visibility, created_at DESC);
CREATE INDEX ix_audit_events_actor_created_at
    ON audit_events (actor_user_id, created_at DESC)
    WHERE actor_user_id IS NOT NULL;
CREATE INDEX ix_audit_events_actor_node_created_at
    ON audit_events (actor_node_id, created_at DESC)
    WHERE actor_node_id IS NOT NULL;
CREATE INDEX ix_audit_events_created_at
    ON audit_events (created_at);
"#;

const DOWN_SQL: &str = r#"
ALTER TABLE deployments
    DROP CONSTRAINT ck_deployments_pending_release_audit_visibility,
    DROP COLUMN pending_release_audit_visibility;

DROP INDEX IF EXISTS ix_audit_events_created_at;
DROP INDEX IF EXISTS ix_audit_events_actor_node_created_at;
DROP INDEX IF EXISTS ix_audit_events_actor_created_at;
DROP INDEX IF EXISTS ix_audit_events_visibility_created_at;
DROP INDEX IF EXISTS ux_audit_events_request_id;

ALTER TABLE audit_events
    DROP CONSTRAINT ck_audit_events_actor_identity,
    DROP CONSTRAINT fk_audit_events_actor_node_id,
    DROP CONSTRAINT ck_audit_events_duration_ms,
    DROP CONSTRAINT ck_audit_events_status_code,
    DROP COLUMN changes,
    DROP COLUMN duration_ms,
    DROP COLUMN status_code,
    DROP COLUMN request_path,
    DROP COLUMN http_method,
    DROP COLUMN user_agent,
    DROP COLUMN source_ip,
    DROP COLUMN request_id,
    DROP COLUMN visibility,
    DROP COLUMN actor_node_id,
    DROP COLUMN actor_type;

DROP TYPE audit_event_visibility;
DROP TYPE audit_actor_type;
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
