use crate::{
    AppMode, AppState, NormalContext, SetupContext, SetupStage, SharedRuntimeMode,
    SharedSetupRuntimeDatabaseConnector,
    domain::setup::{AdminSetupError, AdminSetupErrorKind, InitialAdminInput},
};
use axum::{
    Extension, Json, Router,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use grass_worker_config::{AppConfig, DatabaseConfig, ServerConfig, write_api_database_config};
use grass_worker_database::entities::user;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct SetupStateResponse {
    stage: SetupStage,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct SetupActionResponse {
    stage: SetupStage,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Clone)]
struct SetupRouteState {
    context: SetupContext,
    runtime_mode: Option<SharedRuntimeMode>,
    runtime_database_connector: SharedSetupRuntimeDatabaseConnector,
}

#[derive(Debug, Deserialize)]
struct DatabaseSetupRequest {
    host: String,
    port: u16,
    db_name: String,
    user: String,
    password: String,
    #[serde(default)]
    schema: Option<String>,
}

impl DatabaseSetupRequest {
    fn into_database_config(self) -> DatabaseConfig {
        let schema = self
            .schema
            .as_deref()
            .map(str::trim)
            .filter(|schema| !schema.is_empty())
            .unwrap_or("public")
            .to_owned();

        DatabaseConfig {
            host: self.host,
            port: self.port,
            db_name: self.db_name,
            user: self.user,
            password: self.password,
            schema,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdminSetupRequest {
    email: String,
    password: String,
}

impl AdminSetupRequest {
    fn into_input(self) -> Result<InitialAdminInput, AdminSetupError> {
        let email = self.email.trim().to_ascii_lowercase();
        let password = self.password.trim().to_owned();

        if email.is_empty() {
            return Err(AdminSetupError::validation("email is required"));
        }

        if password.is_empty() {
            return Err(AdminSetupError::validation("password is required"));
        }

        Ok(InitialAdminInput { email, password })
    }
}

async fn database_has_admin(
    connection: &sea_orm::DatabaseConnection,
) -> Result<bool, sea_orm::DbErr> {
    Ok(user::Entity::find()
        .filter(user::Column::IsAdmin.eq(true))
        .one(connection)
        .await?
        .is_some())
}

pub fn install_setup_routes(
    router: Router,
    context: SetupContext,
    runtime_mode: Option<SharedRuntimeMode>,
    runtime_database_connector: SharedSetupRuntimeDatabaseConnector,
) -> Router {
    router
        .route("/api/v1/setup/state", get(setup_state))
        .route("/api/v1/setup/database", post(setup_database))
        .route("/api/v1/setup/admin", post(setup_admin))
        .layer(Extension(SetupRouteState {
            context,
            runtime_mode,
            runtime_database_connector,
        }))
}

async fn setup_state(Extension(state): Extension<SetupRouteState>) -> Json<SetupStateResponse> {
    Json(SetupStateResponse {
        stage: state.context.stage(),
        status: "pending",
    })
}

async fn setup_database(
    Extension(state): Extension<SetupRouteState>,
    payload: Result<Json<DatabaseSetupRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let stage = state.context.stage();
    let database = payload.into_database_config();

    let SetupContext::Database {
        listen,
        config_path,
        initializer,
        development,
        ..
    } = state.context
    else {
        return error_response(StatusCode::CONFLICT, "current setup stage is not database");
    };

    if let Err(error) = initializer.initialize_database(&database).await {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }

    let server = ServerConfig { listen };

    if let Err(error) = write_api_database_config(&config_path, &server, &database) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
    }

    if let Some(runtime_mode) = state.runtime_mode {
        let connection = match state.runtime_database_connector.connect(&database).await {
            Ok(connection) => connection,
            Err(error) => {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
            }
        };

        let next_mode = match database_has_admin(&connection).await {
            Ok(true) => AppMode::Normal(NormalContext::new(
                AppConfig {
                    server,
                    database: Some(database.clone()),
                    development: development.clone(),
                },
                AppState::new(connection),
            )),
            Ok(false) => AppMode::Setup(
                SetupContext::admin(listen, database.clone()).with_development(development),
            ),
            Err(error) => {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
            }
        };

        *runtime_mode.write().await = next_mode;
    }

    (
        StatusCode::OK,
        Json(SetupActionResponse {
            stage,
            status: "completed",
        }),
    )
        .into_response()
}

async fn setup_admin(
    Extension(state): Extension<SetupRouteState>,
    payload: Result<Json<AdminSetupRequest>, JsonRejection>,
) -> impl IntoResponse {
    let SetupContext::Admin {
        listen,
        database,
        creator,
        development,
        ..
    } = state.context
    else {
        return error_response(StatusCode::CONFLICT, "current setup stage is not admin");
    };

    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.to_string()),
    };

    let input = match payload.into_input() {
        Ok(input) => input,
        Err(error) => return admin_error_response(error),
    };

    match creator.create_initial_admin(input).await {
        Ok(()) => {
            if let Some(runtime_mode) = state.runtime_mode {
                let connection = match state.runtime_database_connector.connect(&database).await {
                    Ok(connection) => connection,
                    Err(error) => {
                        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error);
                    }
                };

                *runtime_mode.write().await = AppMode::Normal(NormalContext::new(
                    AppConfig {
                        server: ServerConfig { listen },
                        database: Some(database.clone()),
                        development,
                    },
                    AppState::new(connection),
                ));
            }

            (
                StatusCode::OK,
                Json(SetupActionResponse {
                    stage: SetupStage::Admin,
                    status: "completed",
                }),
            )
                .into_response()
        }
        Err(error) => admin_error_response(error),
    }
}

fn error_response(status: StatusCode, error: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}

fn admin_error_response(error: AdminSetupError) -> axum::response::Response {
    let status = match error.kind() {
        AdminSetupErrorKind::Validation => StatusCode::BAD_REQUEST,
        AdminSetupErrorKind::Conflict => StatusCode::CONFLICT,
        AdminSetupErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };

    error_response(status, error.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppMode, SetupContext, SetupRuntimeDatabaseConnector, app_router,
        domain::setup::{DatabaseInitializer, DatabaseSetupError, InitialAdminCreator},
        runtime_app_router_with_connector,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use chrono::Utc;
    use grass_worker_config::DatabaseConfig;
    use grass_worker_database::entities::user;
    use sea_orm::{DatabaseBackend, DatabaseConnection, MockDatabase, MockDatabaseConnection};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;
    use uuid::Uuid;

    #[derive(Debug)]
    struct FailingDatabaseInitializer;

    #[async_trait::async_trait]
    impl DatabaseInitializer for FailingDatabaseInitializer {
        async fn initialize_database(
            &self,
            _database: &DatabaseConfig,
        ) -> Result<(), DatabaseSetupError> {
            Err(DatabaseSetupError::new("connection refused"))
        }
    }

    #[derive(Debug, Default)]
    struct RecordingDatabaseInitializer {
        last_config: Mutex<Option<DatabaseConfig>>,
    }

    impl RecordingDatabaseInitializer {
        fn success() -> Self {
            Self::default()
        }

        fn last_config(&self) -> Option<DatabaseConfig> {
            self.last_config.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl DatabaseInitializer for RecordingDatabaseInitializer {
        async fn initialize_database(
            &self,
            database: &DatabaseConfig,
        ) -> Result<(), DatabaseSetupError> {
            *self.last_config.lock().unwrap() = Some(database.clone());
            Ok(())
        }
    }

    #[derive(Debug, Default)]
    struct RecordingInitialAdminCreator {
        last_input: Mutex<Option<InitialAdminInput>>,
    }

    impl RecordingInitialAdminCreator {
        fn last_input(&self) -> Option<InitialAdminInput> {
            self.last_input.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl InitialAdminCreator for RecordingInitialAdminCreator {
        async fn create_initial_admin(
            &self,
            input: InitialAdminInput,
        ) -> Result<(), AdminSetupError> {
            *self.last_input.lock().unwrap() = Some(input);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ConflictInitialAdminCreator;

    #[async_trait::async_trait]
    impl InitialAdminCreator for ConflictInitialAdminCreator {
        async fn create_initial_admin(
            &self,
            _input: InitialAdminInput,
        ) -> Result<(), AdminSetupError> {
            Err(AdminSetupError::conflict("initial admin already exists"))
        }
    }

    #[derive(Debug)]
    struct RecordingRuntimeDatabaseConnector {
        last_database: Mutex<Option<DatabaseConfig>>,
        connection: Arc<MockDatabaseConnection>,
    }

    impl RecordingRuntimeDatabaseConnector {
        fn with_connection(connection: MockDatabaseConnection) -> Self {
            Self {
                last_database: Mutex::new(None),
                connection: Arc::new(connection),
            }
        }

        fn last_database(&self) -> Option<DatabaseConfig> {
            self.last_database.lock().unwrap().clone()
        }
    }

    impl Default for RecordingRuntimeDatabaseConnector {
        fn default() -> Self {
            Self::with_connection(MockDatabaseConnection::new(MockDatabase::new(
                DatabaseBackend::Postgres,
            )))
        }
    }

    #[async_trait::async_trait]
    impl SetupRuntimeDatabaseConnector for RecordingRuntimeDatabaseConnector {
        async fn connect(
            &self,
            database: &DatabaseConfig,
        ) -> Result<sea_orm::DatabaseConnection, String> {
            *self.last_database.lock().unwrap() = Some(database.clone());
            Ok(DatabaseConnection::MockDatabaseConnection(
                self.connection.clone(),
            ))
        }
    }

    fn sample_admin_user() -> user::Model {
        let now = Utc::now();

        user::Model {
            id: Uuid::new_v4(),
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn setup_database_returns_bad_request_when_initializer_fails() {
        let app = app_router(AppMode::Setup(SetupContext::database_with_initializer(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
            Arc::new(FailingDatabaseInitializer),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "connection refused");
    }

    #[tokio::test]
    async fn setup_database_persists_config_and_returns_completed_status() {
        let initializer = Arc::new(RecordingDatabaseInitializer::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = app_router(AppMode::Setup(SetupContext::database_with_initializer(
            "127.0.0.1:3000".parse().unwrap(),
            config_path.clone(),
            initializer.clone(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret","schema":"control_plane"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "completed");
        assert_eq!(json["stage"], "database");
        assert_eq!(initializer.last_config().unwrap().schema, "control_plane");

        let written_config = std::fs::read_to_string(config_path).unwrap();
        assert!(written_config.contains("[database]"));
        assert!(written_config.contains("schema = \"control_plane\""));
    }

    #[tokio::test]
    async fn setup_database_defaults_blank_schema_to_public() {
        let initializer = Arc::new(RecordingDatabaseInitializer::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = app_router(AppMode::Setup(SetupContext::database_with_initializer(
            "127.0.0.1:3000".parse().unwrap(),
            config_path,
            initializer.clone(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret","schema":"   "}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(initializer.last_config().unwrap().schema, "public");
    }

    #[tokio::test]
    async fn setup_database_returns_internal_server_error_when_config_persistence_fails() {
        let initializer = Arc::new(RecordingDatabaseInitializer::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config-dir");
        std::fs::create_dir(&config_path).unwrap();
        let app = app_router(AppMode::Setup(SetupContext::database_with_initializer(
            "127.0.0.1:3000".parse().unwrap(),
            config_path,
            initializer,
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("config-dir"));
    }

    #[tokio::test]
    async fn setup_admin_normalizes_email_and_returns_completed_status() {
        let creator = Arc::new(RecordingInitialAdminCreator::default());
        let app = app_router(AppMode::Setup(SetupContext::admin_with_creator(
            "127.0.0.1:3000".parse().unwrap(),
            DatabaseConfig::default(),
            creator.clone(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/admin")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"  ADMIN@Example.com  ","password":"secret-pass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            creator.last_input().unwrap(),
            InitialAdminInput {
                email: "admin@example.com".to_owned(),
                password: "secret-pass".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn setup_admin_rejects_blank_email() {
        let app = app_router(AppMode::Setup(SetupContext::admin(
            "127.0.0.1:3000".parse().unwrap(),
            DatabaseConfig::default(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/admin")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"email":"   ","password":"secret-pass"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn setup_admin_returns_conflict_when_admin_exists() {
        let app = app_router(AppMode::Setup(SetupContext::admin_with_creator(
            "127.0.0.1:3000".parse().unwrap(),
            DatabaseConfig::default(),
            Arc::new(ConflictInitialAdminCreator),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/admin")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"admin@example.com","password":"secret-pass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn runtime_setup_database_request_advances_to_admin_stage() {
        let initializer = Arc::new(RecordingDatabaseInitializer::success());
        let connector = Arc::new(RecordingRuntimeDatabaseConnector::with_connection(
            MockDatabaseConnection::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([Vec::<user::Model>::new()]),
            ),
        ));
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = runtime_app_router_with_connector(
            AppMode::Setup(SetupContext::database_with_initializer(
                "127.0.0.1:3000".parse().unwrap(),
                config_path,
                initializer,
            )),
            connector,
        )
        .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let info = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(info.status(), StatusCode::OK);
        let body = to_bytes(info.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["mode"], "setup");
        assert_eq!(json["stage"], "admin");
    }

    #[tokio::test]
    async fn runtime_setup_database_request_skips_admin_stage_when_initial_admin_exists() {
        let initializer = Arc::new(RecordingDatabaseInitializer::success());
        let connector = Arc::new(RecordingRuntimeDatabaseConnector::with_connection(
            MockDatabaseConnection::new(
                MockDatabase::new(DatabaseBackend::Postgres)
                    .append_query_results([[sample_admin_user()]]),
            ),
        ));
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = runtime_app_router_with_connector(
            AppMode::Setup(SetupContext::database_with_initializer(
                "127.0.0.1:3000".parse().unwrap(),
                config_path,
                initializer,
            )),
            connector,
        )
        .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let info = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(info.status(), StatusCode::OK);
        let body = to_bytes(info.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["mode"], "ready");
        assert!(json.get("stage").is_none());
    }

    #[tokio::test]
    async fn runtime_setup_admin_request_advances_to_ready_mode() {
        let creator = Arc::new(RecordingInitialAdminCreator::default());
        let connector = Arc::new(RecordingRuntimeDatabaseConnector::default());
        let database = DatabaseConfig::default();
        let app = runtime_app_router_with_connector(
            AppMode::Setup(SetupContext::admin_with_creator(
                "127.0.0.1:3000".parse().unwrap(),
                database.clone(),
                creator.clone(),
            )),
            connector.clone(),
        )
        .unwrap();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/setup/admin")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"email":"admin@example.com","password":"secret-pass"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(creator.last_input().unwrap().email, "admin@example.com");
        assert_eq!(connector.last_database().as_ref(), Some(&database));

        let info = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(info.status(), StatusCode::OK);
        let body = to_bytes(info.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["mode"], "ready");
        assert!(json.get("stage").is_none());

        let me = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
    }
}
