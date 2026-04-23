pub mod adapters;
pub mod domain;
mod features;
mod frontend;

use crate::adapters::setup::{PostgresInitialAdminCreator, default_database_initializer};
use crate::domain::{
    setup::{SharedDatabaseInitializer, SharedInitialAdminCreator},
    system::ApiInfo,
};
use axum::routing::any;
use axum::{Json, Router, http::Request, response::Response, routing::get};
use features::{
    auth::install_auth_routes, projects::install_project_routes, setup::install_setup_routes,
    system::install_system_routes,
};
use frontend::{FrontendMode, install_frontend};
use grass_worker_config::AppConfig;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing::Span;

#[derive(Debug, Clone)]
pub struct AppState {
    pub database: Arc<sea_orm::DatabaseConnection>,
    pub auth: crate::adapters::auth::AuthService,
    pub projects: crate::domain::project::ProjectService,
}

impl AppState {
    pub fn new(database: sea_orm::DatabaseConnection) -> Self {
        Self {
            database: Arc::new(database),
            auth: crate::adapters::auth::AuthService::default(),
            projects: crate::domain::project::ProjectService::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NormalContext {
    pub config: AppConfig,
    pub state: AppState,
}

impl NormalContext {
    pub fn new(config: AppConfig, state: AppState) -> Self {
        Self { config, state }
    }
}

#[derive(Debug, Clone)]
pub enum AppMode {
    Normal(NormalContext),
    Setup(SetupContext),
}

impl From<SetupContext> for AppMode {
    fn from(context: SetupContext) -> Self {
        Self::Setup(context)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    Database,
    Admin,
}

#[derive(Clone)]
pub enum SetupContext {
    Database {
        listen: SocketAddr,
        config_path: PathBuf,
        initializer: SharedDatabaseInitializer,
    },
    Admin {
        listen: SocketAddr,
        database: grass_worker_config::DatabaseConfig,
        creator: SharedInitialAdminCreator,
    },
}

impl std::fmt::Debug for SetupContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database {
                listen,
                config_path,
                ..
            } => f
                .debug_struct("SetupContext::Database")
                .field("listen", listen)
                .field("config_path", config_path)
                .finish_non_exhaustive(),
            Self::Admin {
                listen, database, ..
            } => f
                .debug_struct("SetupContext::Admin")
                .field("listen", listen)
                .field("database", database)
                .finish_non_exhaustive(),
        }
    }
}

impl PartialEq for SetupContext {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Database {
                    listen: left_listen,
                    config_path: left_config_path,
                    ..
                },
                Self::Database {
                    listen: right_listen,
                    config_path: right_config_path,
                    ..
                },
            ) => left_listen == right_listen && left_config_path == right_config_path,
            (
                Self::Admin {
                    listen: left_listen,
                    database: left_database,
                    ..
                },
                Self::Admin {
                    listen: right_listen,
                    database: right_database,
                    ..
                },
            ) => left_listen == right_listen && left_database == right_database,
            _ => false,
        }
    }
}

impl Eq for SetupContext {}

impl SetupContext {
    pub fn database(listen: SocketAddr, config_path: PathBuf) -> Self {
        Self::database_with_initializer(listen, config_path, default_database_initializer())
    }

    pub fn database_with_initializer(
        listen: SocketAddr,
        config_path: PathBuf,
        initializer: SharedDatabaseInitializer,
    ) -> Self {
        Self::Database {
            listen,
            config_path,
            initializer,
        }
    }

    pub fn admin(listen: SocketAddr, database: grass_worker_config::DatabaseConfig) -> Self {
        Self::admin_with_creator(
            listen,
            database.clone(),
            std::sync::Arc::new(PostgresInitialAdminCreator::new(database)),
        )
    }

    pub fn admin_with_creator(
        listen: SocketAddr,
        database: grass_worker_config::DatabaseConfig,
        creator: SharedInitialAdminCreator,
    ) -> Self {
        Self::Admin {
            listen,
            database,
            creator,
        }
    }

    pub fn stage(&self) -> SetupStage {
        match self {
            Self::Database { .. } => SetupStage::Database,
            Self::Admin { .. } => SetupStage::Admin,
        }
    }

    pub fn listen(&self) -> SocketAddr {
        match self {
            Self::Database { listen, .. } | Self::Admin { listen, .. } => *listen,
        }
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        service: "control-api",
        status: "ok",
    })
}

async fn api_not_found() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_FOUND
}

fn ensure_rustls_crypto_provider() {
    static PROVIDER: OnceLock<()> = OnceLock::new();

    PROVIDER.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn with_request_logger(router: Router) -> Router {
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    path = %request.uri().path()
                )
            })
            .on_request(|_request: &Request<_>, span: &Span| {
                tracing::info!(parent: span, "request started");
            })
            .on_response(|response: &Response<_>, latency: Duration, span: &Span| {
                tracing::info!(
                    parent: span,
                    status = response.status().as_u16(),
                    latency_ms = latency.as_millis(),
                    "request completed"
                );
            }),
    )
}

pub fn app_router(mode: impl Into<AppMode>) -> std::io::Result<Router> {
    ensure_rustls_crypto_provider();

    let router = Router::new().route("/health", get(health));

    match mode.into() {
        AppMode::Normal(context) => {
            let router = install_system_routes(router, ApiInfo::ready());
            let router = install_auth_routes(router, context.state.clone());
            let router = install_project_routes(router, context.state.clone())
                .route("/api/{*path}", any(api_not_found));
            let frontend_mode = match context.config.development {
                Some(development) => FrontendMode::Development {
                    dev_server: development.dev_server,
                },
                None => FrontendMode::Release {
                    public_dir: std::env::current_dir()?.join("public"),
                },
            };

            Ok(with_request_logger(install_frontend(router, frontend_mode)))
        }
        AppMode::Setup(context) => {
            let stage = context.stage();
            let router = install_system_routes(router, ApiInfo::setup(stage));
            let router =
                install_setup_routes(router, context).route("/api/{*path}", any(api_not_found));
            Ok(with_request_logger(router))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use grass_worker_config::{AppConfig, DatabaseConfig};
    use sea_orm::{DatabaseBackend, MockDatabase};
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use tracing::subscriber::set_default;
    use tracing_subscriber::{fmt, fmt::MakeWriter};

    fn ready_mode() -> AppMode {
        AppMode::Normal(NormalContext::new(
            AppConfig {
                server: grass_worker_config::ServerConfig::default(),
                database: Some(DatabaseConfig::default()),
                development: None,
            },
            AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection()),
        ))
    }

    #[derive(Clone, Default)]
    struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

    struct SharedLogWriter(Arc<Mutex<Vec<u8>>>);

    impl SharedLogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = SharedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedLogWriter(self.0.clone())
        }
    }

    impl Write for SharedLogWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn wait_for_logs(logs: &SharedLogBuffer, needle: &str) -> String {
        for _ in 0..50 {
            let output = logs.contents();
            if output.contains(needle) {
                return output;
            }

            std::thread::yield_now();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        logs.contents()
    }

    #[test]
    fn app_state_new_initializes_auth_service() {
        let state = AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection());

        assert_eq!(state.auth, crate::adapters::auth::AuthService::default());
    }

    #[test]
    fn app_state_new_initializes_project_service() {
        let state = AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection());

        assert_eq!(
            state.projects,
            crate::domain::project::ProjectService::default()
        );
    }

    #[tokio::test]
    async fn health_returns_service_status() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["service"], "control-api");
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_logger_emits_method_path_and_status() {
        let logs = SharedLogBuffer::default();
        let subscriber = fmt()
            .with_writer(logs.clone())
            .with_ansi(false)
            .without_time()
            .finish();
        let _guard = set_default(subscriber);

        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let output = wait_for_logs(&logs, "request completed");
        assert!(output.contains("http_request"));
        assert!(output.contains("method=GET"));
        assert!(output.contains("path=/health"));
        assert!(output.contains("request started"));
        assert!(output.contains("request completed"));
        assert!(output.contains("status=200"));
    }

    #[tokio::test]
    async fn ready_mode_exposes_api_info() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["service"], "control-api");
        assert_eq!(json["mode"], "ready");
        assert!(json.get("stage").is_none());
        assert!(json.get("status").is_none());
    }

    #[tokio::test]
    async fn ready_mode_me_without_cookie_returns_unauthorized() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ready_mode_mounts_project_routes_before_api_fallback() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "missing session");
    }

    #[tokio::test]
    async fn setup_mode_exposes_api_info() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/v1/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["service"], "control-api");
        assert_eq!(json["mode"], "setup");
        assert_eq!(json["stage"], "database");
        assert_eq!(json["status"], "pending");
    }

    #[tokio::test]
    async fn setup_mode_exposes_setup_state_endpoint() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/v1/setup/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["stage"], "database");
        assert_eq!(json["status"], "pending");
    }

    #[tokio::test]
    async fn admin_setup_mode_exposes_api_info() {
        let response = app_router(AppMode::Setup(SetupContext::admin(
            "127.0.0.1:3000".parse().unwrap(),
            DatabaseConfig::default(),
        )))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/v1/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["mode"], "setup");
        assert_eq!(json["stage"], "admin");
        assert_eq!(json["status"], "pending");
    }

    #[tokio::test]
    async fn setup_mode_root_no_longer_returns_html() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn setup_mode_does_not_expose_project_routes() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ready_mode_old_info_path_returns_not_found() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn setup_mode_old_setup_path_returns_not_found() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/setup/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
