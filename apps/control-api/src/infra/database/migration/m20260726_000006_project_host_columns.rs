use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"ALTER TABLE projects ADD COLUMN archived_at TIMESTAMPTZ NULL;
ALTER TABLE project_host_bindings ADD COLUMN is_primary BOOLEAN NOT NULL DEFAULT FALSE;
CREATE UNIQUE INDEX uq_project_host_bindings_primary
    ON project_host_bindings (project_id)
    WHERE is_primary AND deleted_at IS NULL;
ALTER TABLE deployments ADD COLUMN preview_host TEXT NULL;
CREATE UNIQUE INDEX uq_deployments_preview_host
    ON deployments (preview_host)
    WHERE preview_host IS NOT NULL AND deleted_at IS NULL;"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"DROP INDEX IF EXISTS uq_deployments_preview_host;
ALTER TABLE deployments DROP COLUMN preview_host;
DROP INDEX IF EXISTS uq_project_host_bindings_primary;
ALTER TABLE project_host_bindings DROP COLUMN is_primary;
ALTER TABLE projects DROP COLUMN archived_at;"#,
            )
            .await?;

        Ok(())
    }
}
