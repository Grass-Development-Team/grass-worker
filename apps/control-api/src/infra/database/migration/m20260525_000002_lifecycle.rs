use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const TRIGGER_TABLES: &[&str] = &[
    "users",
    "user_password_credentials",
    "quota_plans",
    "team_groups",
    "teams",
    "team_members",
    "team_invitations",
    "nodes",
    "projects",
    "deployments",
    "quota_limits",
    "quota_usage_counters",
    "host_sources",
    "project_host_bindings",
    "host_policies",
    "system_settings",
];

const DELETED_AT_TABLES: &[(&str, &str, &str)] = &[
    ("users", "ux_users_email", "email"),
    ("teams", "ux_teams_slug", "slug"),
    ("projects", "ux_projects_team_slug", "team_id, slug"),
    ("nodes", "ux_nodes_name", "name"),
    (
        "project_host_bindings",
        "ux_project_host_bindings_host",
        "host",
    ),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        install_updated_at_trigger(manager).await?;
        add_before_update_triggers(manager).await?;
        add_deleted_at_columns(manager).await?;
        convert_to_partial_unique_indexes(manager).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        restore_full_unique_indexes(manager).await?;
        remove_deleted_at_columns(manager).await?;
        remove_before_update_triggers(manager).await?;
        uninstall_updated_at_trigger(manager).await?;

        Ok(())
    }
}

async fn install_updated_at_trigger(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute_unprepared(
            r#"CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;"#,
        )
        .await?;

    Ok(())
}

async fn add_before_update_triggers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for table in TRIGGER_TABLES {
        let sql = format!(
            r#"CREATE TRIGGER trg_{table}_updated_at
BEFORE UPDATE ON {table}
FOR EACH ROW
EXECUTE FUNCTION set_updated_at();"#
        );
        manager
            .get_connection()
            .execute_unprepared(&sql)
            .await?;
    }

    Ok(())
}

async fn add_deleted_at_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for &(table, _, _) in DELETED_AT_TABLES {
        let sql = format!(
            "ALTER TABLE {table} ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ"
        );
        manager
            .get_connection()
            .execute_unprepared(&sql)
            .await?;
    }

    Ok(())
}

async fn convert_to_partial_unique_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for &(table, index_name, columns) in DELETED_AT_TABLES {
        let drop_sql = format!("DROP INDEX IF EXISTS {index_name}");
        manager
            .get_connection()
            .execute_unprepared(&drop_sql)
            .await?;

        let create_sql = format!(
            "CREATE UNIQUE INDEX {index_name} ON {table} ({columns}) WHERE deleted_at IS NULL"
        );
        manager
            .get_connection()
            .execute_unprepared(&create_sql)
            .await?;
    }

    Ok(())
}

async fn restore_full_unique_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for &(table, index_name, columns) in DELETED_AT_TABLES {
        let drop_sql = format!("DROP INDEX IF EXISTS {index_name}");
        manager
            .get_connection()
            .execute_unprepared(&drop_sql)
            .await?;

        let create_sql = format!(
            "CREATE UNIQUE INDEX {index_name} ON {table} ({columns})"
        );
        manager
            .get_connection()
            .execute_unprepared(&create_sql)
            .await?;
    }

    Ok(())
}

async fn remove_deleted_at_columns(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for &(table, _, _) in DELETED_AT_TABLES {
        let sql = format!(
            "ALTER TABLE {table} DROP COLUMN IF EXISTS deleted_at"
        );
        manager
            .get_connection()
            .execute_unprepared(&sql)
            .await?;
    }

    Ok(())
}

async fn remove_before_update_triggers(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    for table in TRIGGER_TABLES {
        let sql = format!("DROP TRIGGER IF EXISTS trg_{table}_updated_at ON {table}");
        manager
            .get_connection()
            .execute_unprepared(&sql)
            .await?;
    }

    Ok(())
}

async fn uninstall_updated_at_trigger(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    manager
        .get_connection()
        .execute_unprepared("DROP FUNCTION IF EXISTS set_updated_at()")
        .await?;

    Ok(())
}
