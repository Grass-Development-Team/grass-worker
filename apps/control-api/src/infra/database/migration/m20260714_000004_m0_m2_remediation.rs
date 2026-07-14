use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .add_column(
                        ColumnDef::new(Deployments::RuntimeKind)
                            .custom(Alias::new("project_runtime"))
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                r#"UPDATE deployments AS deployment
SET runtime_kind = project.runtime
FROM projects AS project
WHERE project.id = deployment.project_id"#,
            )
            .await?;
        manager
            .get_connection()
            .execute_unprepared(
                r#"ALTER TABLE deployments
ALTER COLUMN runtime_kind SET DEFAULT 'static'::project_runtime,
ALTER COLUMN runtime_kind SET NOT NULL"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Deployments::Table)
                    .drop_column(Deployments::RuntimeKind)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Deployments {
    Table,
    RuntimeKind,
}
