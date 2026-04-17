use crate::startup::{SetupContext, SetupStage};
use axum::{
    Extension, Json, Router,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use grass_worker_config::{DatabaseConfig, ServerConfig, write_api_database_config};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct SetupStateResponse {
    stage: SetupStage,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct SetupInfoResponse {
    service: &'static str,
    mode: &'static str,
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

pub fn install_setup(router: Router, context: SetupContext) -> Router {
    router
        .route("/api/info", get(setup_info))
        .route("/api/setup/state", get(setup_state))
        .route("/api/setup/database", post(setup_database))
        .layer(Extension(context))
}

async fn setup_info(Extension(context): Extension<SetupContext>) -> Json<SetupInfoResponse> {
    Json(SetupInfoResponse {
        service: "control-api",
        mode: "setup",
        stage: context.stage(),
        status: "pending",
    })
}

async fn setup_state(Extension(context): Extension<SetupContext>) -> Json<SetupStateResponse> {
    Json(SetupStateResponse {
        stage: context.stage(),
        status: "pending",
    })
}

async fn setup_database(
    Extension(context): Extension<SetupContext>,
    payload: Result<Json<DatabaseSetupRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => return error_response(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let stage = context.stage();
    let database = payload.into_database_config();

    let SetupContext::Database {
        listen,
        config_path,
        service,
    } = context
    else {
        return error_response(StatusCode::CONFLICT, "current setup stage is not database");
    };

    if let Err(error) = service.initialize_database(&database).await {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }

    let server = ServerConfig { listen };

    if let Err(error) = write_api_database_config(&config_path, &server, &database) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string());
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

fn error_response(status: StatusCode, error: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{DatabaseSetupError, DatabaseSetupService};
    use crate::{AppMode, app_router};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use grass_worker_config::DatabaseConfig;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[derive(Debug)]
    struct FailingSetupService;

    #[async_trait::async_trait]
    impl DatabaseSetupService for FailingSetupService {
        async fn initialize_database(
            &self,
            _database: &DatabaseConfig,
        ) -> Result<(), DatabaseSetupError> {
            Err(DatabaseSetupError::new("connection refused"))
        }
    }

    #[derive(Debug, Default)]
    struct RecordingSetupService {
        last_config: Mutex<Option<DatabaseConfig>>,
    }

    impl RecordingSetupService {
        fn success() -> Self {
            Self::default()
        }

        fn last_config(&self) -> Option<DatabaseConfig> {
            self.last_config.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl DatabaseSetupService for RecordingSetupService {
        async fn initialize_database(
            &self,
            database: &DatabaseConfig,
        ) -> Result<(), DatabaseSetupError> {
            *self.last_config.lock().unwrap() = Some(database.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn setup_database_returns_bad_request_when_initializer_fails() {
        let app = app_router(AppMode::Setup(SetupContext::database_with_service(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
            Arc::new(FailingSetupService),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/setup/database")
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
        let service = Arc::new(RecordingSetupService::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = app_router(AppMode::Setup(SetupContext::database_with_service(
            "127.0.0.1:3000".parse().unwrap(),
            config_path.clone(),
            service.clone(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/setup/database")
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
        assert_eq!(service.last_config().unwrap().schema, "control_plane");

        let written_config = std::fs::read_to_string(config_path).unwrap();
        assert!(written_config.contains("[database]"));
        assert!(written_config.contains("schema = \"control_plane\""));
    }

    #[tokio::test]
    async fn setup_database_defaults_blank_schema_to_public() {
        let service = Arc::new(RecordingSetupService::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        let app = app_router(AppMode::Setup(SetupContext::database_with_service(
            "127.0.0.1:3000".parse().unwrap(),
            config_path,
            service.clone(),
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/setup/database")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"host":"127.0.0.1","port":5432,"db_name":"grass_worker","user":"postgres","password":"secret","schema":"   "}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(service.last_config().unwrap().schema, "public");
    }

    #[tokio::test]
    async fn setup_database_returns_internal_server_error_when_config_persistence_fails() {
        let service = Arc::new(RecordingSetupService::success());
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("config-dir");
        std::fs::create_dir(&config_path).unwrap();
        let app = app_router(AppMode::Setup(SetupContext::database_with_service(
            "127.0.0.1:3000".parse().unwrap(),
            config_path,
            service,
        )))
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/setup/database")
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
}
