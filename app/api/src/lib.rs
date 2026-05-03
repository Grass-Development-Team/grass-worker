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
    auth::install_auth_routes, deployments::install_deployment_routes,
    projects::install_project_routes, releases::install_release_routes,
    setup::install_setup_routes, system::install_system_routes, users::install_user_routes,
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
    pub projects: crate::domain::project::ProjectService,
    pub hosts: crate::domain::host::HostService,
    pub deployments: crate::domain::deployment::DeploymentService,
    pub releases: crate::domain::release::ReleaseService,
    pub sites: crate::domain::site::SiteService,
    pub users: crate::domain::user::UserService,
}

impl AppState {
    pub fn new(database: sea_orm::DatabaseConnection) -> Self {
        Self {
            database: Arc::new(database),
            auth: crate::adapters::auth::AuthService,
            projects: crate::domain::project::ProjectService,
            hosts: crate::domain::host::HostService,
            deployments: crate::domain::deployment::DeploymentService,
            releases: crate::domain::release::ReleaseService,
            sites: crate::domain::site::SiteService,
            users: crate::domain::user::UserService,
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
            let router = install_project_routes(router, context.state.clone());
            let router = install_deployment_routes(router, context.state.clone());
            let router = install_release_routes(router, context.state.clone());
            let router = install_user_routes(router, context.state.clone())
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
#[test]
fn normalize_host_strips_port_and_trailing_dot() {
    assert_eq!(
        crate::domain::host::normalize_host("  Docs.Example.com.:443  ").unwrap(),
        "docs.example.com"
    );
}

#[cfg(test)]
#[tokio::test]
async fn create_binding_promotes_first_binding_to_primary() {
    use chrono::{TimeZone, Utc};
    use grass_worker_database::entities::{project, project_host_binding};
    use sea_orm::{DatabaseBackend, MockDatabase, MockExecResult};
    use uuid::Uuid;

    fn actor(id: Uuid) -> crate::domain::auth::AuthenticatedUser {
        crate::domain::auth::AuthenticatedUser {
            id,
            email: "owner@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
        }
    }

    fn sample_project(id: Uuid, owner_user_id: Uuid) -> project::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 9, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id,
            active_deployment_id: None,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: project::ProjectStatus::Active,
            created_at: now,
            updated_at: now,
            archived_at: None,
            soft_deleted_at: None,
        }
    }

    let owner_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let database = MockDatabase::new(DatabaseBackend::Postgres)
        .append_query_results([[sample_project(project_id, owner_id)]])
        .append_query_results([Vec::<project_host_binding::Model>::new()])
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();

    let binding = crate::domain::host::HostService
        .create_for_project(
            &database,
            &actor(owner_id),
            project_id,
            crate::domain::host::CreateProjectHostInput {
                source_id: None,
                host: " Docs.Example.com ".to_owned(),
                is_primary: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(binding.host, "docs.example.com");
    assert!(binding.is_primary);
}

#[cfg(test)]
#[tokio::test]
async fn resolve_by_host_returns_active_ready_static_site() {
    use chrono::{TimeZone, Utc};
    use grass_worker_database::entities::{
        deployment, deployment_artifact, project, project_host_binding,
    };
    use sea_orm::{DatabaseBackend, MockDatabase};
    use uuid::Uuid;

    fn sample_project(id: Uuid, active_deployment_id: Option<Uuid>) -> project::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 0, 0).unwrap();
        project::Model {
            id,
            owner_user_id: Uuid::new_v4(),
            active_deployment_id,
            slug: "docs-site".to_owned(),
            name: "Docs Site".to_owned(),
            status: project::ProjectStatus::Active,
            created_at: now,
            updated_at: now,
            archived_at: None,
            soft_deleted_at: None,
        }
    }

    fn sample_binding(project_id: Uuid) -> project_host_binding::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 5, 0).unwrap();
        project_host_binding::Model {
            id: Uuid::new_v4(),
            project_id,
            source_id: None,
            host: "docs.example.com".to_owned(),
            is_primary: true,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_deployment(id: Uuid, project_id: Uuid) -> deployment::Model {
        let now = Utc.with_ymd_and_hms(2026, 5, 3, 10, 10, 0).unwrap();
        deployment::Model {
            id,
            project_id,
            status: deployment::DeploymentStatus::Ready,
            source_branch: Some("main".to_owned()),
            source_revision: Some("deadbeef".to_owned()),
            created_at: now,
            started_at: Some(now),
            finished_at: Some(now),
        }
    }

    let project_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();
    let database = MockDatabase::new(DatabaseBackend::Postgres)
        .append_query_results([[sample_binding(project_id)]])
        .append_query_results([[sample_project(project_id, Some(deployment_id))]])
        .append_query_results([[sample_deployment(deployment_id, project_id)]])
        .append_query_results([[deployment_artifact::Model {
            id: Uuid::new_v4(),
            deployment_id,
            kind: deployment_artifact::ArtifactKind::StaticSite,
            storage_path: "/tmp/docs-site".to_owned(),
            checksum_sha256: Some("abc123".to_owned()),
            size_bytes: Some(1024),
            created_at: Utc.with_ymd_and_hms(2026, 5, 3, 10, 15, 0).unwrap(),
        }]])
        .into_connection();

    let resolved = crate::domain::site::SiteService
        .resolve_by_host(&database, " Docs.Example.com.:443 ")
        .await
        .unwrap();

    assert_eq!(
        resolved,
        Some(crate::domain::site::ResolvedSite {
            project_id,
            project_slug: "docs-site".to_owned(),
            host: "docs.example.com".to_owned(),
            root_dir: "/tmp/docs-site".to_owned(),
        })
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use grass_worker_config::{AppConfig, DatabaseConfig};
    use grass_worker_database::entities::{
        deployment, deployment_artifact, project, project_host_binding, user, user_session,
    };
    use sea_orm::{DatabaseBackend, MockDatabase};
    use std::fs;
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use tempfile::tempdir;
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

    #[test]
    fn app_state_new_initializes_project_service() {
        let state = AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection());

        assert_eq!(state.projects, crate::domain::project::ProjectService);
    }

    #[test]
    fn app_state_new_initializes_host_service() {
        let state = AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection());

        assert_eq!(state.hosts, crate::domain::host::HostService);
    }

    #[test]
    fn app_state_new_initializes_site_service() {
        let state = AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection());

        assert_eq!(state.sites, crate::domain::site::SiteService);
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
        let now = chrono::Utc::now();
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
        let now = chrono::Utc::now();
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
                active_deployment_id: None,
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
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
    async fn ready_mode_activate_release_returns_release_envelope() {
        let now = chrono::Utc::now();
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let project_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let deployment_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
        let session_token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap(),
                user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[user::Model {
                id: user_id,
                email: "owner@example.com".to_owned(),
                is_admin: false,
                is_initial_admin: false,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([[project::Model {
                id: project_id,
                owner_user_id: user_id,
                active_deployment_id: None,
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .append_query_results([[deployment::Model {
                id: deployment_id,
                project_id,
                status: deployment::DeploymentStatus::Ready,
                source_branch: Some("main".to_owned()),
                source_revision: Some("deadbeef".to_owned()),
                created_at: now,
                started_at: Some(now),
                finished_at: Some(now),
            }]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::parse_str("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee").unwrap(),
                deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/tmp/docs-site".to_owned(),
                checksum_sha256: Some("abc123".to_owned()),
                size_bytes: Some(1024),
                created_at: now,
            }]])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .append_query_results([[project::Model {
                id: project_id,
                owner_user_id: user_id,
                active_deployment_id: Some(deployment_id),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .append_query_results([[deployment::Model {
                id: deployment_id,
                project_id,
                status: deployment::DeploymentStatus::Ready,
                source_branch: Some("main".to_owned()),
                source_revision: Some("deadbeef".to_owned()),
                created_at: now,
                started_at: Some(now),
                finished_at: Some(now),
            }]])
            .append_query_results([Vec::<deployment::Model>::new()])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/release/activate"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::COOKIE,
                        format!(
                            "{}={session_token}",
                            crate::domain::auth::SESSION_COOKIE_NAME
                        ),
                    )
                    .body(Body::from(format!(
                        r#"{{"deployment_id":"{deployment_id}"}}"#
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["release"]["project_id"], project_id.to_string());
        assert_eq!(json["release"]["project_slug"], "docs-site");
        assert_eq!(
            json["release"]["active_deployment_id"],
            deployment_id.to_string()
        );
        assert_eq!(
            json["release"]["active_deployment"]["id"],
            deployment_id.to_string()
        );
        assert_eq!(json["release"]["site_url"], "/sites/docs-site");
    }

    #[tokio::test]
    async fn ready_mode_rollback_release_returns_previous_ready_static_release() {
        let now = chrono::Utc::now();
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let project_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let current_deployment_id =
            Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
        let previous_deployment_id =
            Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();
        let session_token = "session-token";
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee").unwrap(),
                user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[user::Model {
                id: user_id,
                email: "owner@example.com".to_owned(),
                is_admin: false,
                is_initial_admin: false,
                created_at: now,
                updated_at: now,
            }]])
            .append_query_results([[project::Model {
                id: project_id,
                owner_user_id: user_id,
                active_deployment_id: Some(current_deployment_id),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .append_query_results([vec![
                deployment::Model {
                    id: current_deployment_id,
                    project_id,
                    status: deployment::DeploymentStatus::Ready,
                    source_branch: Some("main".to_owned()),
                    source_revision: Some("current".to_owned()),
                    created_at: now,
                    started_at: Some(now),
                    finished_at: Some(now),
                },
                deployment::Model {
                    id: previous_deployment_id,
                    project_id,
                    status: deployment::DeploymentStatus::Ready,
                    source_branch: Some("main".to_owned()),
                    source_revision: Some("previous".to_owned()),
                    created_at: now - chrono::Duration::hours(1),
                    started_at: Some(now - chrono::Duration::hours(1)),
                    finished_at: Some(now - chrono::Duration::hours(1)),
                },
            ]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap(),
                deployment_id: previous_deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/tmp/docs-site-previous".to_owned(),
                checksum_sha256: Some("prev".to_owned()),
                size_bytes: Some(1024),
                created_at: now,
            }]])
            .append_exec_results([sea_orm::MockExecResult {
                last_insert_id: 0,
                rows_affected: 1,
            }])
            .append_query_results([[project::Model {
                id: project_id,
                owner_user_id: user_id,
                active_deployment_id: Some(previous_deployment_id),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .append_query_results([Vec::<project_host_binding::Model>::new()])
            .append_query_results([[deployment::Model {
                id: previous_deployment_id,
                project_id,
                status: deployment::DeploymentStatus::Ready,
                source_branch: Some("main".to_owned()),
                source_revision: Some("previous".to_owned()),
                created_at: now - chrono::Duration::hours(1),
                started_at: Some(now - chrono::Duration::hours(1)),
                finished_at: Some(now - chrono::Duration::hours(1)),
            }]])
            .append_query_results([vec![
                deployment::Model {
                    id: current_deployment_id,
                    project_id,
                    status: deployment::DeploymentStatus::Ready,
                    source_branch: Some("main".to_owned()),
                    source_revision: Some("current".to_owned()),
                    created_at: now,
                    started_at: Some(now),
                    finished_at: Some(now),
                },
                deployment::Model {
                    id: previous_deployment_id,
                    project_id,
                    status: deployment::DeploymentStatus::Ready,
                    source_branch: Some("main".to_owned()),
                    source_revision: Some("previous".to_owned()),
                    created_at: now - chrono::Duration::hours(1),
                    started_at: Some(now - chrono::Duration::hours(1)),
                    finished_at: Some(now - chrono::Duration::hours(1)),
                },
            ]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap(),
                deployment_id: current_deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: "/tmp/docs-site-current".to_owned(),
                checksum_sha256: Some("current".to_owned()),
                size_bytes: Some(2048),
                created_at: now,
            }]])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/projects/{project_id}/release/rollback"))
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

        assert_eq!(
            json["release"]["active_deployment_id"],
            previous_deployment_id.to_string()
        );
        assert_eq!(
            json["release"]["rollback_deployment_id"],
            current_deployment_id.to_string()
        );
    }

    #[tokio::test]
    async fn ready_mode_serves_active_static_site_with_spa_fallback() {
        let now = chrono::Utc::now();
        let project_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let deployment_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
        let site_dir = tempdir().unwrap();
        fs::write(
            site_dir.path().join("index.html"),
            "<html>Published Docs Site</html>",
        )
        .unwrap();

        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[project::Model {
                id: project_id,
                owner_user_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
                active_deployment_id: Some(deployment_id),
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .append_query_results([[deployment::Model {
                id: deployment_id,
                project_id,
                status: deployment::DeploymentStatus::Ready,
                source_branch: Some("main".to_owned()),
                source_revision: Some("deadbeef".to_owned()),
                created_at: now,
                started_at: Some(now),
                finished_at: Some(now),
            }]])
            .append_query_results([[deployment_artifact::Model {
                id: Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap(),
                deployment_id,
                kind: deployment_artifact::ArtifactKind::StaticSite,
                storage_path: site_dir.path().to_string_lossy().into_owned(),
                checksum_sha256: Some("abc123".to_owned()),
                size_bytes: Some(1024),
                created_at: now,
            }]])
            .into_connection();

        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/sites/docs-site/docs/getting-started")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("Published Docs Site")
        );
    }

    #[tokio::test]
    async fn ready_mode_project_details_with_cookie_returns_owned_project() {
        let now = chrono::Utc::now();
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session_token = "session-token";
        let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
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
                id: project_id,
                owner_user_id: user_id,
                active_deployment_id: None,
                slug: "docs-site".to_owned(),
                name: "Docs Site".to_owned(),
                status: project::ProjectStatus::Active,
                created_at: now,
                updated_at: now,
                archived_at: None,
                soft_deleted_at: None,
            }]])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project_id}"))
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

        assert_eq!(json["project"]["id"], project_id.to_string());
        assert_eq!(json["project"]["slug"], "docs-site");
        assert_eq!(json["project"]["name"], "Docs Site");
    }

    #[tokio::test]
    async fn ready_mode_project_details_returns_not_found_for_missing_project() {
        let now = chrono::Utc::now();
        let user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let session_token = "session-token";
        let project_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
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
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/projects/{project_id}"))
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

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ready_mode_admin_users_with_cookie_returns_user_list() {
        let now = chrono::Utc::now();
        let admin_user_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let member_user_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let session_token = "session-token";
        let admin_user = user::Model {
            id: admin_user_id,
            email: "admin@example.com".to_owned(),
            is_admin: true,
            is_initial_admin: true,
            created_at: now,
            updated_at: now,
        };
        let member_user = user::Model {
            id: member_user_id,
            email: "member@example.com".to_owned(),
            is_admin: false,
            is_initial_admin: false,
            created_at: now,
            updated_at: now,
        };
        let database = MockDatabase::new(DatabaseBackend::Postgres)
            .append_query_results([[user_session::Model {
                id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap(),
                user_id: admin_user_id,
                token_hash: crate::adapters::auth::hash_session_token(session_token),
                created_at: now,
                expires_at: now + chrono::Duration::days(7),
                revoked_at: None,
            }]])
            .append_query_results([[admin_user.clone()]])
            .append_query_results([vec![admin_user.clone(), member_user.clone()]])
            .into_connection();
        let response = app_router(ready_mode_with_database(database))
            .unwrap()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/users")
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

        assert_eq!(json["users"].as_array().unwrap().len(), 2);
        assert_eq!(json["users"][0]["email"], "admin@example.com");
        assert_eq!(json["users"][1]["email"], "member@example.com");
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
