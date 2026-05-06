use grass_worker_database::entities::{platform_host_source, project, project_host_binding};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(platform_host_source::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(platform_host_source::Column::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::Kind)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::Label)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::BaseDomain)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::Enabled)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::AllowsAutoAssign)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(platform_host_source::Column::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(project_host_binding::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(project_host_binding::Column::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(project_host_binding::Column::ProjectId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(project_host_binding::Column::SourceId).uuid())
                    .col(
                        ColumnDef::new(project_host_binding::Column::Host)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(project_host_binding::Column::IsPrimary)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(project_host_binding::Column::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(project_host_binding::Column::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-project-host-bindings-project-id")
                            .from(
                                project_host_binding::Entity,
                                project_host_binding::Column::ProjectId,
                            )
                            .to(project::Entity, project::Column::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-project-host-bindings-source-id")
                            .from(
                                project_host_binding::Entity,
                                project_host_binding::Column::SourceId,
                            )
                            .to(
                                platform_host_source::Entity,
                                platform_host_source::Column::Id,
                            )
                            .on_delete(ForeignKeyAction::SetNull)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("uq-project-host-bindings-host")
                    .table(project_host_binding::Entity)
                    .col(project_host_binding::Column::Host)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-project-host-bindings-project-id")
                    .table(project_host_binding::Entity)
                    .col(project_host_binding::Column::ProjectId)
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE UNIQUE INDEX uq_project_host_bindings_primary_per_project
                ON project_host_bindings (project_id)
                WHERE is_primary = true
                "#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
                DROP INDEX IF EXISTS "uq_project_host_bindings_primary_per_project"
                "#,
            )
            .await?;

        manager
            .drop_table(Table::drop().table(project_host_binding::Entity).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(platform_host_source::Entity).to_owned())
            .await
    }
}
