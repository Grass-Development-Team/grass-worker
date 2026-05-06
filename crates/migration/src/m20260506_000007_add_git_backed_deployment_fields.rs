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
                    .add_column(ColumnDef::new(project::Column::RepositoryUrl).string().null())
                    .add_column(
                        ColumnDef::new(project::Column::ProductionBranch)
                            .string()
                            .null(),
                    )
                    .add_column(ColumnDef::new(project::Column::RootDirectory).string().null())
                    .add_column(ColumnDef::new(project::Column::InstallCommand).string().null())
                    .add_column(ColumnDef::new(project::Column::BuildCommand).string().null())
                    .add_column(ColumnDef::new(project::Column::OutputDirectory).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(deployment::Entity)
                    .add_column(ColumnDef::new(deployment::Column::LastStage).string().null())
                    .add_column(
                        ColumnDef::new(deployment::Column::FailureMessage)
                            .string()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(deployment::Entity)
                    .drop_column(deployment::Column::FailureMessage)
                    .drop_column(deployment::Column::LastStage)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(project::Entity)
                    .drop_column(project::Column::OutputDirectory)
                    .drop_column(project::Column::BuildCommand)
                    .drop_column(project::Column::InstallCommand)
                    .drop_column(project::Column::RootDirectory)
                    .drop_column(project::Column::ProductionBranch)
                    .drop_column(project::Column::RepositoryUrl)
                    .to_owned(),
            )
            .await
    }
}
