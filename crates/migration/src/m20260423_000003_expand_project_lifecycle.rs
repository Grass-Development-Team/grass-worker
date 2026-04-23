use grass_worker_database::entities::project;
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
                        ColumnDef::new(project::Column::SoftDeletedAt).timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(project::Entity)
                    .drop_column(project::Column::SoftDeletedAt)
                    .to_owned(),
            )
            .await
    }
}
