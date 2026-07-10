use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(TeamInvitations::Table)
                    .add_column(ColumnDef::new(TeamInvitations::TokenHash).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .get_connection()
            .execute_unprepared(
                "CREATE UNIQUE INDEX ux_team_invitations_token_hash \
                 ON team_invitations (token_hash) WHERE token_hash IS NOT NULL",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("ux_team_invitations_token_hash")
                    .table(TeamInvitations::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(TeamInvitations::Table)
                    .drop_column(TeamInvitations::TokenHash)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum TeamInvitations {
    Table,
    TokenHash,
}
