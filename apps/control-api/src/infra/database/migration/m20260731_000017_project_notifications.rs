use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
ALTER TABLE projects
    ADD COLUMN created_by_user_id UUID NULL;

UPDATE projects AS project
SET created_by_user_id = creator.actor_user_id
FROM (
    SELECT DISTINCT ON (target_id)
        target_id AS project_id,
        actor_user_id
    FROM audit_events
    WHERE action = 'project.created'
      AND target_type = 'project'
      AND result = 'success'
      AND target_id IS NOT NULL
      AND actor_user_id IS NOT NULL
    ORDER BY target_id, created_at ASC, id ASC
) AS creator
WHERE project.id = creator.project_id;

ALTER TABLE projects
    ADD CONSTRAINT fk_projects_created_by_user_id
        FOREIGN KEY (created_by_user_id) REFERENCES users (id) ON DELETE SET NULL;

CREATE INDEX ix_projects_created_by_user_id
    ON projects (created_by_user_id)
    WHERE created_by_user_id IS NOT NULL;

CREATE TABLE user_notifications (
    id UUID PRIMARY KEY,
    recipient_user_id UUID NOT NULL,
    actor_user_id UUID NULL,
    team_id UUID NULL,
    project_id UUID NULL,
    action TEXT NOT NULL,
    project_name TEXT NOT NULL,
    project_slug TEXT NOT NULL,
    actor_label TEXT NOT NULL,
    reason TEXT NULL,
    target_url TEXT NOT NULL,
    read_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_user_notifications_recipient_user_id
        FOREIGN KEY (recipient_user_id) REFERENCES users (id) ON DELETE CASCADE,
    CONSTRAINT fk_user_notifications_actor_user_id
        FOREIGN KEY (actor_user_id) REFERENCES users (id) ON DELETE SET NULL,
    CONSTRAINT fk_user_notifications_team_id
        FOREIGN KEY (team_id) REFERENCES teams (id) ON DELETE SET NULL,
    CONSTRAINT fk_user_notifications_project_id
        FOREIGN KEY (project_id) REFERENCES projects (id) ON DELETE SET NULL
);

CREATE INDEX ix_user_notifications_recipient_created
    ON user_notifications (recipient_user_id, created_at DESC, id DESC);

CREATE INDEX ix_user_notifications_recipient_unread
    ON user_notifications (recipient_user_id, created_at DESC, id DESC)
    WHERE read_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP TABLE user_notifications;

DROP INDEX ix_projects_created_by_user_id;

ALTER TABLE projects
    DROP CONSTRAINT fk_projects_created_by_user_id,
    DROP COLUMN created_by_user_id;
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
    fn migration_adds_project_creator_and_persistent_recipient_notifications() {
        assert!(UP_SQL.contains("ADD COLUMN created_by_user_id UUID NULL"));
        assert!(!UP_SQL.contains("created_by_user_id UUID NULL DEFAULT"));
        assert!(UP_SQL.contains("action = 'project.created'"));
        assert!(UP_SQL.contains("target_type = 'project'"));
        assert!(UP_SQL.contains("actor_user_id IS NOT NULL"));
        assert!(UP_SQL.contains("fk_projects_created_by_user_id"));
        assert!(UP_SQL.contains("REFERENCES users (id) ON DELETE SET NULL"));

        assert!(UP_SQL.contains("CREATE TABLE user_notifications"));
        assert!(UP_SQL.contains("recipient_user_id UUID NOT NULL"));
        assert!(UP_SQL.contains("actor_user_id UUID NULL"));
        assert!(UP_SQL.contains("team_id UUID NULL"));
        assert!(UP_SQL.contains("project_id UUID NULL"));
        assert!(UP_SQL.contains("reason TEXT NULL"));
        assert!(UP_SQL.contains("read_at TIMESTAMPTZ NULL"));
        assert!(!UP_SQL.contains("read_at TIMESTAMPTZ NULL DEFAULT"));
        assert!(UP_SQL.contains("fk_user_notifications_recipient_user_id"));
        assert!(UP_SQL.contains("ON DELETE CASCADE"));
        assert!(UP_SQL.contains("ix_user_notifications_recipient_created"));
        assert!(UP_SQL.contains("ix_user_notifications_recipient_unread"));
        assert!(UP_SQL.contains("WHERE read_at IS NULL"));
    }

    #[test]
    fn notification_migration_is_reversible() {
        assert!(DOWN_SQL.contains("DROP TABLE user_notifications"));
        assert!(DOWN_SQL.contains("DROP COLUMN created_by_user_id"));
    }
}
