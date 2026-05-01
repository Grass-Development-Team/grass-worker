use grass_worker_database::entities::{deployment, project};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(project::Entity)
                    .add_column(
                        ColumnDef::new(project::Column::ActiveDeploymentId)
                            .uuid()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-projects-active-deployment-id")
                    .table(project::Entity)
                    .col(project::Column::ActiveDeploymentId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk-projects-active-deployment-id")
                    .from(project::Entity, project::Column::ActiveDeploymentId)
                    .to(deployment::Entity, deployment::Column::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk-projects-active-deployment-id")
                    .table(project::Entity)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx-projects-active-deployment-id")
                    .table(project::Entity)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(project::Entity)
                    .drop_column(project::Column::ActiveDeploymentId)
                    .to_owned(),
            )
            .await
    }
}
