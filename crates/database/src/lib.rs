pub mod connection;
pub mod entities;
pub mod repository;

#[cfg(test)]
mod tests {
    use super::connection::{create_schema_sql, postgres_connection_string, set_search_path_sql};
    use super::entities::{deployment, deployment_artifact, project};
    use super::repository::{
        DeploymentArtifactRepository, DeploymentRepository, NewDeployment, NewDeploymentArtifact,
        NewProject, ProjectRepository, SeaOrmDeploymentArtifactRepository,
        SeaOrmDeploymentRepository, SeaOrmProjectRepository,
    };
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};
    use sea_orm::EntityName;
    use grass_worker_config::DatabaseConfig;
    use uuid::Uuid;

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

    #[tokio::test]
    async fn project_repository_create_returns_active_project() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmProjectRepository::new(database);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let project = repository
            .create(NewProject {
                id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                created_at,
            })
            .await
            .unwrap();

        assert_eq!(project.slug, "docs-site");
        assert_eq!(project.name, "Docs Site");
        assert_eq!(project.status, project::ProjectStatus::Active);
        assert_eq!(project.created_at, created_at);
        assert_eq!(project.updated_at, created_at);
        assert_eq!(project.archived_at, None);
    }

    #[tokio::test]
    async fn deployment_repository_lists_project_deployments() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let deployment = deployment::Model {
            id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
            project_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            status: deployment::DeploymentStatus::Pending,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            created_at,
            started_at: None,
            finished_at: None,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[deployment.clone()]])
            .into_connection();
        let repository = SeaOrmDeploymentRepository::new(database);

        let deployments = repository
            .list_by_project(deployment.project_id)
            .await
            .unwrap();

        assert_eq!(deployments, vec![deployment]);
    }

    #[tokio::test]
    async fn deployment_repository_create_returns_pending_deployment() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmDeploymentRepository::new(database);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let deployment = repository
            .create(NewDeployment {
                id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                project_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
                source_branch: Some("main".to_owned()),
                source_revision: Some("deadbeef".to_owned()),
                created_at,
            })
            .await
            .unwrap();

        assert_eq!(deployment.status, deployment::DeploymentStatus::Pending);
        assert_eq!(deployment.created_at, created_at);
        assert_eq!(deployment.started_at, None);
        assert_eq!(deployment.finished_at, None);
    }

    #[tokio::test]
    async fn deployment_artifact_repository_creates_artifact() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmDeploymentArtifactRepository::new(database);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let artifact = repository
            .create(NewDeploymentArtifact {
                id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap(),
                deployment_id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/deployments/bbbbbbbb/static-site.tar".to_owned(),
                checksum_sha256: Some("abc123".to_owned()),
                size_bytes: Some(2048),
                created_at,
            })
            .await
            .unwrap();

        assert_eq!(artifact.kind, deployment_artifact::ArtifactKind::StaticSite);
        assert_eq!(artifact.size_bytes, Some(2048));
        assert_eq!(artifact.created_at, created_at);
    }
}
