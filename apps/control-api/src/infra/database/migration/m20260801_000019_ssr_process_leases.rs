use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE ssr_process_leases (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL,
    team_id UUID NOT NULL,
    node_id UUID NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    renewed_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    hour_block_start TIMESTAMPTZ NOT NULL,
    released_at TIMESTAMPTZ NULL,
    CONSTRAINT fk_ssr_process_leases_deployment_id
        FOREIGN KEY (deployment_id) REFERENCES deployments (id) ON DELETE CASCADE,
    CONSTRAINT fk_ssr_process_leases_team_id
        FOREIGN KEY (team_id) REFERENCES teams (id) ON DELETE CASCADE,
    CONSTRAINT fk_ssr_process_leases_node_id
        FOREIGN KEY (node_id) REFERENCES nodes (id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX uq_ssr_process_leases_active_deployment_node
    ON ssr_process_leases (deployment_id, node_id)
    WHERE released_at IS NULL;

CREATE INDEX ix_ssr_process_leases_team_active
    ON ssr_process_leases (team_id, expires_at)
    WHERE released_at IS NULL;
"#;

pub(crate) const DOWN_SQL: &str = r#"
DROP INDEX ix_ssr_process_leases_team_active;
DROP INDEX uq_ssr_process_leases_active_deployment_node;
DROP TABLE ssr_process_leases;
"#;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP_SQL).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(DOWN_SQL)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_expiring_process_leases_and_active_indexes() {
        assert!(UP_SQL.contains("CREATE TABLE ssr_process_leases"));
        assert!(UP_SQL.contains("hour_block_start TIMESTAMPTZ NOT NULL"));
        assert!(UP_SQL.contains("released_at TIMESTAMPTZ NULL"));
        assert!(UP_SQL.contains("WHERE released_at IS NULL"));
        assert!(DOWN_SQL.contains("DROP TABLE ssr_process_leases"));
    }
}
