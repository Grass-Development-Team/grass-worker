pub mod connection;
pub mod entities;
pub mod repository;

#[cfg(test)]
mod tests {
    use super::connection::{create_schema_sql, postgres_connection_string, set_search_path_sql};
    use super::entities::{
        deployment, deployment_artifact, project, user, user_password_credential, user_session,
    };
    use super::repository::{
        DeploymentArtifactRepository, DeploymentRepository, NewDeployment, NewDeploymentArtifact,
        NewProject, NewUser, NewUserPasswordCredential, NewUserSession, ProjectRepository,
        SeaOrmDeploymentArtifactRepository, SeaOrmDeploymentRepository, SeaOrmProjectRepository,
        SeaOrmUserPasswordCredentialRepository, SeaOrmUserRepository, SeaOrmUserSessionRepository,
        UserPasswordCredentialRepository, UserRepository, UserSessionRepository,
        find_password_credential_by_user_id, find_session_by_token_hash, find_user_by_email,
        find_user_by_id, insert_session, revoke_session_by_token_hash,
    };
    use grass_worker_config::DatabaseConfig;
    use sea_orm::EntityName;
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};
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
        assert_eq!(
            deployment_artifact::Entity.table_name(),
            "deployment_artifacts"
        );
    }

    #[test]
    fn user_entity_uses_expected_table_name() {
        assert_eq!(user::Entity.table_name(), "users");
    }

    #[test]
    fn user_password_credential_entity_uses_expected_table_name() {
        assert_eq!(
            user_password_credential::Entity.table_name(),
            "user_password_credentials"
        );
    }

    #[test]
    fn user_session_entity_uses_expected_table_name() {
        assert_eq!(user_session::Entity.table_name(), "user_sessions");
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
                owner_user_id: Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap(),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                created_at,
            })
            .await
            .unwrap();

        assert_eq!(project.slug, "docs-site");
        assert_eq!(project.name, "Docs Site");
        assert_eq!(
            project.owner_user_id,
            Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap()
        );
        assert_eq!(project.status, project::ProjectStatus::Active);
        assert_eq!(project.created_at, created_at);
        assert_eq!(project.updated_at, created_at);
        assert_eq!(project.archived_at, None);
    }

    #[tokio::test]
    async fn project_repository_lists_projects_by_owner() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let project = project::Model {
            id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            owner_user_id: Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap(),
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: project::ProjectStatus::Active,
            created_at,
            updated_at: created_at,
            archived_at: None,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[project.clone()]])
            .into_connection();
        let repository = SeaOrmProjectRepository::new(database);

        let projects = repository
            .list_by_owner(project.owner_user_id)
            .await
            .unwrap();

        assert_eq!(projects, vec![project]);
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

    #[tokio::test]
    async fn user_repository_create_returns_non_admin_user() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmUserRepository::new(database);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let user = repository
            .create(NewUser {
                id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                email: "user@example.com".to_owned(),
                created_at,
            })
            .await
            .unwrap();

        assert_eq!(user.email, "user@example.com");
        assert!(!user.is_admin);
        assert!(!user.is_initial_admin);
        assert_eq!(user.created_at, created_at);
        assert_eq!(user.updated_at, created_at);
    }

    #[tokio::test]
    async fn user_repository_create_admin_returns_initial_admin() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmUserRepository::new(database);
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let user = repository
            .create_admin(
                NewUser {
                    id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                    email: "admin@example.com".to_owned(),
                    created_at,
                },
                true,
            )
            .await
            .unwrap();

        assert!(user.is_admin);
        assert!(user.is_initial_admin);
        assert_eq!(user.email, "admin@example.com");
    }

    #[tokio::test]
    async fn user_repository_reports_admin_presence() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-17T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let admin = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[admin]])
            .into_connection();
        let repository = SeaOrmUserRepository::new(database);

        assert!(repository.has_admin().await.unwrap());
    }

    #[tokio::test]
    async fn user_repository_finds_user_by_email() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "user@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at,
            updated_at: created_at,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user.clone()]])
            .into_connection();
        let repository = SeaOrmUserRepository::new(database);

        let found = repository.find_by_email("user@example.com").await.unwrap();

        assert_eq!(found, Some(user));
    }

    #[tokio::test]
    async fn password_credential_repository_sets_password_hash() {
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let repository = SeaOrmUserPasswordCredentialRepository::new(database);
        let password_updated_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        let credential = repository
            .set_password(NewUserPasswordCredential {
                user_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                password_hash: "argon2id$example".to_owned(),
                password_updated_at,
            })
            .await
            .unwrap();

        assert_eq!(credential.password_hash, "argon2id$example");
        assert_eq!(credential.password_updated_at, password_updated_at);
    }

    #[tokio::test]
    async fn auth_repository_helpers_find_records_for_login_and_session_lookup() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let expires_at = chrono::DateTime::parse_from_rfc3339("2026-04-23T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let user = user::Model {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at,
            updated_at: created_at,
        };
        let credential = user_password_credential::Model {
            user_id: user.id,
            password_hash: "$argon2id$example".to_owned(),
            password_updated_at: created_at,
        };
        let session = user_session::Model {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            user_id: user.id,
            token_hash: "sha256$session".to_owned(),
            created_at,
            expires_at,
            revoked_at: None,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user.clone()]])
            .append_query_results([[user.clone()]])
            .append_query_results([[credential.clone()]])
            .append_query_results([[session.clone()]])
            .into_connection();

        assert_eq!(
            find_user_by_email(&database, "admin@example.com")
                .await
                .unwrap(),
            Some(user.clone())
        );
        assert_eq!(
            find_user_by_id(&database, user.id).await.unwrap(),
            Some(user)
        );
        assert_eq!(
            find_password_credential_by_user_id(&database, credential.user_id)
                .await
                .unwrap(),
            Some(credential)
        );
        assert_eq!(
            find_session_by_token_hash(&database, "sha256$session")
                .await
                .unwrap(),
            Some(session)
        );
    }

    #[tokio::test]
    async fn auth_repository_helpers_insert_and_revoke_session() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let expires_at = chrono::DateTime::parse_from_rfc3339("2026-04-23T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([
                MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
                MockExecResult {
                    last_insert_id: 0,
                    rows_affected: 1,
                },
            ])
            .into_connection();
        let session = user_session::Model {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            user_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            token_hash: "sha256$session".to_owned(),
            created_at,
            expires_at,
            revoked_at: None,
        };

        insert_session(&database, &session).await.unwrap();
        revoke_session_by_token_hash(&database, &session.token_hash, created_at)
            .await
            .unwrap();

        let transaction_log = database.into_transaction_log();
        let statements = transaction_log
            .iter()
            .flat_map(|entry| entry.statements().iter())
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains("INSERT INTO \"user_sessions\""))
        );
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains("UPDATE \"user_sessions\""))
        );
        assert!(
            statements
                .iter()
                .any(|statement| statement.contains("'sha256$session'"))
        );
    }

    #[tokio::test]
    async fn session_repository_creates_and_reads_session_by_token_hash() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-16T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let expires_at = chrono::DateTime::parse_from_rfc3339("2026-04-23T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                user_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                token_hash: "sha256$session".to_owned(),
                created_at,
                expires_at,
                revoked_at: None,
            }]])
            .into_connection();
        let repository = SeaOrmUserSessionRepository::new(database);

        let session = repository
            .create(NewUserSession {
                id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                user_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                token_hash: "sha256$session".to_owned(),
                created_at,
                expires_at,
            })
            .await
            .unwrap();
        let found = repository
            .find_by_token_hash("sha256$session")
            .await
            .unwrap();

        assert_eq!(session.token_hash, "sha256$session");
        assert_eq!(session.expires_at, expires_at);
        assert_eq!(found.unwrap().id, session.id);
    }
}
