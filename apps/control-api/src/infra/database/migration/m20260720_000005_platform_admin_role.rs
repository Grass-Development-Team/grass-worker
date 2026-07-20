use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"CREATE TYPE platform_role AS ENUM ('user', 'admin');
ALTER TABLE users ADD COLUMN platform_role platform_role NOT NULL DEFAULT 'user';
WITH initial_admin AS (
    SELECT id
    FROM users
    WHERE deleted_at IS NULL
    ORDER BY created_at ASC, id ASC
    LIMIT 1
)
UPDATE users
SET platform_role = 'admin'::platform_role
FROM initial_admin
WHERE users.id = initial_admin.id;"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"ALTER TABLE users DROP COLUMN platform_role;
DROP TYPE platform_role;"#,
            )
            .await?;

        Ok(())
    }
}
