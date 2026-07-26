use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"ALTER TABLE audit_events ADD COLUMN team_id UUID NULL;
CREATE INDEX ix_audit_events_team_id ON audit_events (team_id, created_at DESC);
CREATE INDEX ix_audit_events_action ON audit_events (action, created_at DESC);"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"DROP INDEX IF EXISTS ix_audit_events_action;
DROP INDEX IF EXISTS ix_audit_events_team_id;
ALTER TABLE audit_events DROP COLUMN team_id;"#,
            )
            .await?;

        Ok(())
    }
}
