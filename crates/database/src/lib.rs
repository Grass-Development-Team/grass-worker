pub mod connection;
pub mod entities;

#[cfg(test)]
mod tests {
    use super::connection::{create_schema_sql, postgres_connection_string, set_search_path_sql};
    use super::entities::{deployment, deployment_artifact, project};
    use sea_orm::EntityName;
    use grass_worker_config::DatabaseConfig;

    #[test]
    fn project_entity_uses_expected_table_name() {
        assert_eq!(project::Entity.table_name(), "projects");
    }

    #[test]
    fn deployment_entity_uses_expected_table_name() {
        assert_eq!(deployment::Entity.table_name(), "deployments");
    }

    #[test]
    fn deployment_artifact_entity_uses_expected_table_name() {
        assert_eq!(deployment_artifact::Entity.table_name(), "deployment_artifacts");
    }

    #[test]
    fn postgres_connection_string_uses_structured_config() {
        let config = DatabaseConfig {
            host: "db.internal".to_owned(),
            port: 15432,
            db_name: "grass_worker".to_owned(),
            user: "grass".to_owned(),
            password: "secret".to_owned(),
            schema: "control_plane".to_owned(),
        };

        assert_eq!(
            postgres_connection_string(&config),
            "postgres://grass:secret@db.internal:15432/grass_worker"
        );
    }

    #[test]
    fn schema_sql_targets_configured_schema() {
        assert_eq!(
            create_schema_sql("control_plane").unwrap(),
            r#"CREATE SCHEMA IF NOT EXISTS "control_plane""#
        );
        assert_eq!(
            set_search_path_sql("control_plane").unwrap(),
            r#"SET search_path TO "control_plane""#
        );
    }

    #[test]
    fn schema_sql_rejects_invalid_schema_name() {
        let error = create_schema_sql("control-plane").unwrap_err();

        assert!(error.to_string().contains("invalid postgres schema name"));
    }
}
