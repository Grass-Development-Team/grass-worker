use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE announcements (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    auto_popup BOOLEAN NOT NULL DEFAULT false,
    created_by_user_id UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    published_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT ck_announcements_title_length
        CHECK (btrim(title) <> '' AND char_length(title) BETWEEN 1 AND 120),
    CONSTRAINT ck_announcements_content_length
        CHECK (btrim(content) <> '' AND char_length(content) BETWEEN 1 AND 10000)
);

CREATE INDEX ix_announcements_published_at
    ON announcements (published_at DESC, id DESC);

ALTER TABLE user_notifications
    ADD COLUMN announcement_id UUID NULL REFERENCES announcements(id) ON DELETE CASCADE;

INSERT INTO announcements (
    id, title, content, auto_popup, created_by_user_id, published_at
)
SELECT
    gen_random_uuid(),
    n.title,
    n.content,
    false,
    n.actor_user_id,
    n.created_at
FROM user_notifications n
WHERE n.action = 'site.announcement'
  AND n.title IS NOT NULL
  AND n.content IS NOT NULL
GROUP BY n.title, n.content, n.actor_user_id, n.created_at;

INSERT INTO announcements (
    id, title, content, auto_popup, created_by_user_id, published_at
)
SELECT
    gen_random_uuid(),
    current_values.title,
    current_values.content,
    false,
    NULL,
    current_values.published_at
FROM (
    SELECT
        max(value #>> '{}') FILTER (WHERE key = 'site.announcement.title') AS title,
        max(value #>> '{}') FILTER (WHERE key = 'site.announcement.content') AS content,
        max(updated_at) AS published_at
    FROM system_settings
    WHERE key IN ('site.announcement.title', 'site.announcement.content')
) current_values
WHERE current_values.title IS NOT NULL
  AND btrim(current_values.title) <> ''
  AND current_values.content IS NOT NULL
  AND btrim(current_values.content) <> ''
  AND NOT EXISTS (
      SELECT 1
      FROM announcements existing
      WHERE existing.title = current_values.title
        AND existing.content = current_values.content
  );

UPDATE user_notifications n
SET announcement_id = a.id
FROM announcements a
WHERE n.action = 'site.announcement'
  AND n.title = a.title
  AND n.content = a.content
  AND n.created_at = a.published_at
  AND n.actor_user_id IS NOT DISTINCT FROM a.created_by_user_id;

ALTER TABLE user_notifications
    DROP CONSTRAINT ck_user_notifications_announcement_content;

ALTER TABLE user_notifications
    ADD CONSTRAINT ck_user_notifications_announcement_content
        CHECK (
            (
                action = 'site.announcement'
                AND announcement_id IS NOT NULL
                AND title IS NOT NULL
                AND btrim(title) <> ''
                AND content IS NOT NULL
                AND btrim(content) <> ''
                AND team_id IS NULL
                AND project_id IS NULL
            )
            OR (
                action <> 'site.announcement'
                AND announcement_id IS NULL
            )
        );

DELETE FROM system_settings
WHERE key IN ('site.announcement.title', 'site.announcement.content');
"#;

pub(crate) const DOWN_SQL: &str = r#"
WITH latest AS (
    SELECT title, content, published_at
    FROM announcements
    ORDER BY published_at DESC, id DESC
    LIMIT 1
)
INSERT INTO system_settings (
    id, key, value_kind, value, is_secret, created_at, updated_at
)
SELECT gen_random_uuid(), 'site.announcement.title', 'string', to_jsonb(title), false,
       published_at, published_at
FROM latest
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    value_kind = EXCLUDED.value_kind,
    updated_at = EXCLUDED.updated_at;

WITH latest AS (
    SELECT title, content, published_at
    FROM announcements
    ORDER BY published_at DESC, id DESC
    LIMIT 1
)
INSERT INTO system_settings (
    id, key, value_kind, value, is_secret, created_at, updated_at
)
SELECT gen_random_uuid(), 'site.announcement.content', 'string', to_jsonb(content), false,
       published_at, published_at
FROM latest
ON CONFLICT (key) DO UPDATE
SET value = EXCLUDED.value,
    value_kind = EXCLUDED.value_kind,
    updated_at = EXCLUDED.updated_at;

ALTER TABLE user_notifications
    DROP CONSTRAINT ck_user_notifications_announcement_content;

UPDATE user_notifications
SET announcement_id = NULL
WHERE announcement_id IS NOT NULL;

ALTER TABLE user_notifications
    DROP COLUMN announcement_id;

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

DROP TABLE announcements;
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
    fn migration_creates_history_and_links_notification_rows() {
        assert!(UP_SQL.contains("CREATE TABLE announcements"));
        assert!(UP_SQL.contains("auto_popup BOOLEAN NOT NULL DEFAULT false"));
        assert!(UP_SQL.contains("ADD COLUMN announcement_id UUID NULL"));
        assert!(UP_SQL.contains("ON DELETE CASCADE"));
        assert!(UP_SQL.contains("GROUP BY n.title, n.content, n.actor_user_id, n.created_at"));
        assert!(UP_SQL.contains("DELETE FROM system_settings"));
        assert!(UP_SQL.contains("announcement_id IS NOT NULL"));
    }

    #[test]
    fn migration_restores_legacy_settings_and_notification_shape_on_down() {
        assert!(DOWN_SQL.contains("site.announcement.title"));
        assert!(DOWN_SQL.contains("site.announcement.content"));
        assert!(DOWN_SQL.contains("UPDATE user_notifications"));
        assert!(DOWN_SQL.contains("DROP COLUMN announcement_id"));
        assert!(DOWN_SQL.contains("DROP TABLE announcements"));
    }
}
