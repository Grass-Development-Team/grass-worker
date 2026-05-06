use grass_worker_database::entities::host_policy;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(host_policy::Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(host_policy::Column::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(host_policy::Column::MaxHostsPerProject)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(host_policy::Column::MaxHostsPerOwnerUser)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(host_policy::Column::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(host_policy::Column::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(host_policy::Entity).to_owned())
            .await
    }
}
