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
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::Request,
    response::{IntoResponse, Response},
    routing::get,
};
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
use tower::util::ServiceExt;
use tower_http::trace::TraceLayer;
use tracing::Span;

#[derive(Debug, Clone)]
pub struct AppState {
    pub database: Arc<sea_orm::DatabaseConnection>,
    pub auth: crate::adapters::auth::AuthService,
}

impl AppState {
    pub fn new(database: sea_orm::DatabaseConnection) -> Self {
        Self {
            database: Arc::new(database),
            auth: crate::adapters::auth::AuthService,
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
        development: Option<grass_worker_config::DevelopmentConfig>,
    },
    Admin {
        listen: SocketAddr,
        database: grass_worker_config::DatabaseConfig,
        creator: SharedInitialAdminCreator,
        development: Option<grass_worker_config::DevelopmentConfig>,
    },
}

impl std::fmt::Debug for SetupContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database {
                listen,
                config_path,
                development,
                ..
            } => f
                .debug_struct("SetupContext::Database")
                .field("listen", listen)
                .field("config_path", config_path)
                .field("development", development)
                .finish_non_exhaustive(),
            Self::Admin {
                listen,
                database,
                development,
                ..
            } => f
                .debug_struct("SetupContext::Admin")
                .field("listen", listen)
                .field("database", database)
                .field("development", development)
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
                    development: left_development,
                    ..
                },
                Self::Database {
                    listen: right_listen,
                    config_path: right_config_path,
                    development: right_development,
                    ..
                },
            ) => {
                left_listen == right_listen
                    && left_config_path == right_config_path
                    && left_development == right_development
            }
            (
                Self::Admin {
                    listen: left_listen,
                    database: left_database,
                    development: left_development,
                    ..
                },
                Self::Admin {
                    listen: right_listen,
                    database: right_database,
                    development: right_development,
                    ..
                },
            ) => {
                left_listen == right_listen
                    && left_database == right_database
                    && left_development == right_development
            }
            _ => false,
        }
    }
}

impl Eq for SetupContext {}

#[async_trait::async_trait]
pub trait SetupRuntimeDatabaseConnector: Send + Sync {
    async fn connect(
        &self,
        database: &grass_worker_config::DatabaseConfig,
    ) -> Result<sea_orm::DatabaseConnection, String>;
}

pub type SharedSetupRuntimeDatabaseConnector = Arc<dyn SetupRuntimeDatabaseConnector>;
pub type SharedRuntimeMode = Arc<tokio::sync::RwLock<AppMode>>;

#[derive(Debug)]
struct PreparedSetupRuntimeDatabaseConnector;

#[async_trait::async_trait]
impl SetupRuntimeDatabaseConnector for PreparedSetupRuntimeDatabaseConnector {
    async fn connect(
        &self,
        database: &grass_worker_config::DatabaseConfig,
    ) -> Result<sea_orm::DatabaseConnection, String> {
        crate::adapters::database::connect_runtime_database(database).await
    }
}

fn default_setup_runtime_database_connector() -> SharedSetupRuntimeDatabaseConnector {
    Arc::new(PreparedSetupRuntimeDatabaseConnector)
}

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
            development: None,
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
            development: None,
        }
    }

    pub fn with_development(
        self,
        development: Option<grass_worker_config::DevelopmentConfig>,
    ) -> Self {
        match self {
            Self::Database {
                listen,
                config_path,
                initializer,
                ..
            } => Self::Database {
                listen,
                config_path,
                initializer,
                development,
            },
            Self::Admin {
                listen,
                database,
                creator,
                ..
            } => Self::Admin {
                listen,
                database,
                creator,
                development,
            },
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

    pub fn development(&self) -> Option<&grass_worker_config::DevelopmentConfig> {
        match self {
            Self::Database { development, .. } | Self::Admin { development, .. } => {
                development.as_ref()
            }
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

fn resolve_frontend_mode(
    development: Option<&grass_worker_config::DevelopmentConfig>,
) -> std::io::Result<FrontendMode> {
    Ok(match development {
        Some(development) => FrontendMode::Development {
            dev_server: development.dev_server.clone(),
        },
        None => FrontendMode::Release {
            public_dir: std::env::current_dir()?.join("public"),
        },
    })
}

fn build_app_router(
    mode: AppMode,
    runtime_mode: Option<SharedRuntimeMode>,
    runtime_database_connector: SharedSetupRuntimeDatabaseConnector,
) -> std::io::Result<Router> {
    let router = Router::new().route("/health", get(health));

    match mode {
        AppMode::Normal(context) => {
            let router = install_system_routes(router, ApiInfo::ready());
            let router = install_auth_routes(router, context.state.clone());
            let router = install_project_routes(router, context.state.clone())
                .route("/api/{*path}", any(api_not_found));
            let frontend_mode = resolve_frontend_mode(context.config.development.as_ref())?;

            Ok(with_request_logger(install_frontend(router, frontend_mode)))
        }
        AppMode::Setup(context) => {
            let stage = context.stage();
            let frontend_mode = resolve_frontend_mode(context.development())?;
            let router = install_system_routes(router, ApiInfo::setup(stage));
            let router =
                install_setup_routes(router, context, runtime_mode, runtime_database_connector)
                    .route("/api/{*path}", any(api_not_found));

            Ok(install_frontend(router, frontend_mode))
        }
    }
}

#[derive(Clone)]
struct RuntimeRouterState {
    mode: SharedRuntimeMode,
    runtime_database_connector: SharedSetupRuntimeDatabaseConnector,
}

async fn dispatch_runtime_request(
    State(state): State<RuntimeRouterState>,
    request: Request<Body>,
) -> Response {
    let mode = state.mode.read().await.clone();
    let router = match build_app_router(
        mode,
        Some(state.mode.clone()),
        state.runtime_database_connector.clone(),
    ) {
        Ok(router) => router,
        Err(error) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                error.to_string(),
            )
                .into_response();
        }
    };

    match router.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

pub fn app_router(mode: impl Into<AppMode>) -> std::io::Result<Router> {
    ensure_rustls_crypto_provider();

    Ok(with_request_logger(build_app_router(
        mode.into(),
        None,
        default_setup_runtime_database_connector(),
    )?))
}

pub fn runtime_app_router(mode: impl Into<AppMode>) -> std::io::Result<Router> {
    runtime_app_router_with_connector(mode.into(), default_setup_runtime_database_connector())
}

pub(crate) fn runtime_app_router_with_connector(
    mode: AppMode,
    runtime_database_connector: SharedSetupRuntimeDatabaseConnector,
) -> std::io::Result<Router> {
    ensure_rustls_crypto_provider();

    let state = RuntimeRouterState {
        mode: Arc::new(tokio::sync::RwLock::new(mode)),
        runtime_database_connector,
    };

    Ok(with_request_logger(
        Router::new()
            .fallback(dispatch_runtime_request)
            .with_state(state),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
        routing::get,
    };
    use grass_worker_config::{AppConfig, DatabaseConfig, DevelopmentConfig};
    use grass_worker_database::entities::{project, user, user_session};
    use sea_orm::{DatabaseBackend, MockDatabase};
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tower::ServiceExt;
    use tracing::subscriber::set_default;
    use tracing_subscriber::{fmt, fmt::MakeWriter};
    use uuid::Uuid;

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

    fn ready_mode_with_database(database: sea_orm::DatabaseConnection) -> AppMode {
        AppMode::Normal(NormalContext::new(
            AppConfig {
                server: grass_worker_config::ServerConfig::default(),
                database: Some(DatabaseConfig::default()),
                development: None,
            },
            AppState::new(database),
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

        assert_eq!(state.auth, crate::adapters::auth::AuthService);
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
    async fn ready_mode_projects_without_cookie_returns_unauthorized() {
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
    }

    #[tokio::test]
    async fn ready_mode_projects_with_cookie_returns_owned_projects() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session_token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[user::Model {
                id: user_id,
                email: "admin@example.com".to_owned(),
                is_admin: true,
                is_initial_admin: true,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([[
                project::Model {
                    id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                    owner_user_id: user_id,
                    slug: "docs-site".to_owned(),
                    name: "Docs Site".to_owned(),
                    status: project::ProjectStatus::Active,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                },
                project::Model {
                    id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                    owner_user_id: user_id,
                    slug: "legacy-console".to_owned(),
                    name: "Legacy Console".to_owned(),
                    status: project::ProjectStatus::Archived,
                    created_at: now,
                    updated_at: now,
                    archived_at: Some(now),
                },
            ]])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/projects")
                    .header(
                        header::COOKIE,
                        format!(
                            "{}={session_token}",
                            crate::domain::auth::SESSION_COOKIE_NAME
                        ),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["projects"][0]["slug"], "docs-site");
        assert_eq!(json["projects"][0]["name"], "Docs Site");
        assert_eq!(json["projects"][0]["status"], "active");
        assert_eq!(json["projects"][1]["slug"], "legacy-console");
        assert_eq!(json["projects"][1]["status"], "archived");
    }

    #[tokio::test]
    async fn ready_mode_create_project_without_cookie_returns_unauthorized() {
        let response = app_router(ready_mode())
            .unwrap()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"name":"Docs Site","slug":"docs-site"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ready_mode_create_project_with_cookie_returns_created_project() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session_token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[user::Model {
                id: user_id,
                email: "admin@example.com".to_owned(),
                is_admin: true,
                is_initial_admin: true,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([Vec::<project::Model>::new()])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::COOKIE,
                        format!(
                            "{}={session_token}",
                            crate::domain::auth::SESSION_COOKIE_NAME
                        ),
                    )
                    .body(Body::from(r#"{"name":"Docs Site","slug":"docs-site"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["project"]["slug"], "docs-site");
        assert_eq!(json["project"]["name"], "Docs Site");
        assert_eq!(json["project"]["status"], "active");
    }

    #[tokio::test]
    async fn ready_mode_create_project_returns_conflict_for_duplicate_slug() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-04-19T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session_token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
                user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[user::Model {
                id: user_id,
                email: "admin@example.com".to_owned(),
                is_admin: true,
                is_initial_admin: true,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([[project::Model {
                id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                owner_user_id: user_id,
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
            }]])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::COOKIE,
                        format!(
                            "{}={session_token}",
                            crate::domain::auth::SESSION_COOKIE_NAME
                        ),
                    )
                    .body(Body::from(r#"{"name":"Docs Site","slug":"docs-site"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
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
    async fn setup_mode_root_serves_frontend_html() {
        let response = app_router(AppMode::Setup(SetupContext::database(
            "127.0.0.1:3000".parse().unwrap(),
            PathBuf::from("config.toml"),
        )))
        .unwrap()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(html.contains("Grass Worker Console"));
        assert!(html.contains(r#"<div id="app"></div>"#));
    }

    #[tokio::test]
    async fn setup_mode_with_development_config_proxies_frontend_requests() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = upstream.local_addr().unwrap();
        let upstream_app = Router::new().route("/", get(|| async { "frontend dev server" }));
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream, upstream_app).await.unwrap();
        });

        let response = app_router(AppMode::Setup(
            SetupContext::database(
                "127.0.0.1:3000".parse().unwrap(),
                PathBuf::from("config.toml"),
            )
            .with_development(Some(DevelopmentConfig {
                dev_server: format!("http://{address}"),
            })),
        ))
        .unwrap()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

        upstream_task.abort();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(std::str::from_utf8(&body).unwrap(), "frontend dev server");
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
