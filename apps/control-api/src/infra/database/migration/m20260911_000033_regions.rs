use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

pub(crate) const UP_SQL: &str = r#"
CREATE TABLE regions (
    code TEXT PRIMARY KEY CHECK (code ~ '^[a-z0-9_-]{1,64}$'),
    name TEXT NOT NULL CHECK (char_length(btrim(name)) BETWEEN 1 AND 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO regions (code, name)
SELECT region, region FROM (
    SELECT 'default' AS region UNION SELECT region FROM nodes
    UNION SELECT region FROM deployments UNION SELECT region FROM regional_ingresses
    UNION SELECT region FROM host_sources UNION SELECT region FROM project_host_bindings
    UNION SELECT desired_config #>> '{node,region}' FROM nodes
    UNION SELECT effective_config #>> '{node,region}' FROM nodes
) existing WHERE region IS NOT NULL;
ALTER TABLE nodes ADD CONSTRAINT fk_nodes_region FOREIGN KEY (region) REFERENCES regions(code);
ALTER TABLE deployments ADD CONSTRAINT fk_deployments_region FOREIGN KEY (region) REFERENCES regions(code);
ALTER TABLE regional_ingresses ADD CONSTRAINT fk_regional_ingresses_region FOREIGN KEY (region) REFERENCES regions(code);
ALTER TABLE host_sources ADD CONSTRAINT fk_host_sources_region FOREIGN KEY (region) REFERENCES regions(code);
ALTER TABLE project_host_bindings ADD CONSTRAINT fk_project_host_bindings_region FOREIGN KEY (region) REFERENCES regions(code);
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
            .execute_unprepared(
                r#"
ALTER TABLE nodes DROP CONSTRAINT fk_nodes_region;
ALTER TABLE deployments DROP CONSTRAINT fk_deployments_region;
ALTER TABLE regional_ingresses DROP CONSTRAINT fk_regional_ingresses_region;
ALTER TABLE host_sources DROP CONSTRAINT fk_host_sources_region;
ALTER TABLE project_host_bindings DROP CONSTRAINT fk_project_host_bindings_region;
DROP TABLE regions;
"#,
            )
            .await?;
        Ok(())
    }
}
