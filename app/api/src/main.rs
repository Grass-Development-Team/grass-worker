use grass_worker_api::{
    AppMode, AppState, NormalContext, SetupContext,
    adapters::{
        database::connect_runtime_database,
        setup::{PostgresInitialAdminCreator, default_setup_bootstrapper},
    },
    app_router,
    domain::setup::SharedSetupBootstrapper,
    runtime_app_router,
};
use grass_worker_config::{DatabaseConfig, ResolvedApiConfig};
use std::process::ExitCode;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolveAppModeError {
    message: String,
}

impl ResolveAppModeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ResolveAppModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ResolveAppModeError {}

#[async_trait::async_trait]
trait RuntimeDatabaseConnector: Send + Sync {
    async fn connect(
        &self,
        database: &DatabaseConfig,
    ) -> Result<sea_orm::DatabaseConnection, ResolveAppModeError>;
}

type SharedRuntimeDatabaseConnector = Arc<dyn RuntimeDatabaseConnector>;

#[derive(Debug)]
struct PreparedRuntimeDatabaseConnector;

#[async_trait::async_trait]
impl RuntimeDatabaseConnector for PreparedRuntimeDatabaseConnector {
    async fn connect(
        &self,
        database: &DatabaseConfig,
    ) -> Result<sea_orm::DatabaseConnection, ResolveAppModeError> {
        connect_runtime_database(database)
            .await
            .map_err(ResolveAppModeError::new)
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("grass_worker_api=info,tower_http=info"));

    let _ = fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .try_init();
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    match resolve_app_mode(
        ResolvedApiConfig::load()?,
        default_setup_bootstrapper(),
        Arc::new(PreparedRuntimeDatabaseConnector),
    )
    .await?
    {
        AppMode::Normal(context) => {
            let listener = tokio::net::TcpListener::bind(context.config.server.listen).await?;
            let app = app_router(AppMode::Normal(context))?;

            axum::serve(listener, app).await?;
        }
        AppMode::Setup(context) => {
            let listener = tokio::net::TcpListener::bind(context.listen()).await?;
            let app = runtime_app_router(AppMode::Setup(context))?;

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

async fn resolve_app_mode(
    resolved: grass_worker_config::ResolvedApiConfig,
    bootstrapper: SharedSetupBootstrapper,
    runtime_database_connector: SharedRuntimeDatabaseConnector,
) -> Result<AppMode, ResolveAppModeError> {
    let Some(database) = resolved.config.database.clone() else {
        return Ok(AppMode::Setup(
            SetupContext::database(resolved.config.server.listen, resolved.path)
                .with_development(resolved.config.development.clone()),
        ));
    };

    if bootstrapper
        .initialize_and_has_admin(&database)
        .await
        .map_err(|error| ResolveAppModeError::new(error.to_string()))?
    {
        let database_connection = runtime_database_connector.connect(&database).await?;
        return Ok(AppMode::Normal(NormalContext::new(
            resolved.config,
            AppState::new(database_connection),
        )));
    }

    Ok(AppMode::Setup(
        SetupContext::admin_with_creator(
            resolved.config.server.listen,
            database.clone(),
            Arc::new(PostgresInitialAdminCreator::new(database)),
        )
        .with_development(resolved.config.development.clone()),
    ))
}

#[cfg(test)]
mod tests {
    use super::{ResolveAppModeError, RuntimeDatabaseConnector, resolve_app_mode};
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use grass_worker_api::{
        app_router,
        domain::setup::{SetupBootstrapError, SetupBootstrapper},
    };
    use grass_worker_config::{AppConfig, DatabaseConfig, ResolvedApiConfig};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[derive(Debug)]
    struct StubSetupBootstrapper {
        has_admin: bool,
    }

    #[derive(Debug)]
    struct StubRuntimeDatabaseConnector;

    #[async_trait::async_trait]
    impl SetupBootstrapper for StubSetupBootstrapper {
        async fn initialize_and_has_admin(
            &self,
            _database: &DatabaseConfig,
        ) -> Result<bool, SetupBootstrapError> {
            Ok(self.has_admin)
        }
    }

    #[async_trait::async_trait]
    impl RuntimeDatabaseConnector for StubRuntimeDatabaseConnector {
        async fn connect(
            &self,
            _database: &DatabaseConfig,
        ) -> Result<sea_orm::DatabaseConnection, ResolveAppModeError> {
            Ok(sea_orm::MockDatabase::new(sea_orm::DatabaseBackend::Postgres).into_connection())
        }
    }

    #[tokio::test]
    async fn resolve_app_mode_uses_database_stage_when_database_config_is_missing() {
        let mode = resolve_app_mode(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig::defaults(),
            },
            Arc::new(StubSetupBootstrapper { has_admin: false }),
            Arc::new(StubRuntimeDatabaseConnector),
        )
        .await
        .unwrap();

        let response = app_router(mode)
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
        assert_eq!(json["stage"], "database");
    }

    #[tokio::test]
    async fn resolve_app_mode_uses_admin_stage_when_database_exists_without_admin() {
        let mode = resolve_app_mode(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig {
                    server: grass_worker_config::ServerConfig::default(),
                    database: Some(DatabaseConfig::default()),
                    development: None,
                },
            },
            Arc::new(StubSetupBootstrapper { has_admin: false }),
            Arc::new(StubRuntimeDatabaseConnector),
        )
        .await
        .unwrap();

        let response = app_router(mode)
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
    }

    #[tokio::test]
    async fn resolve_app_mode_uses_ready_mode_when_admin_exists() {
        let mode = resolve_app_mode(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig {
                    server: grass_worker_config::ServerConfig::default(),
                    database: Some(DatabaseConfig::default()),
                    development: None,
                },
            },
            Arc::new(StubSetupBootstrapper { has_admin: true }),
            Arc::new(StubRuntimeDatabaseConnector),
        )
        .await
        .unwrap();

        let response = app_router(mode)
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
        assert_eq!(json["mode"], "ready");
        assert!(json.get("stage").is_none());
    }

    #[tokio::test]
    async fn resolve_app_mode_builds_normal_context_when_admin_exists() {
        let mode = resolve_app_mode(
            ResolvedApiConfig {
                path: PathBuf::from("config.toml"),
                config: AppConfig {
                    server: grass_worker_config::ServerConfig::default(),
                    database: Some(DatabaseConfig::default()),
                    development: None,
                },
            },
            Arc::new(StubSetupBootstrapper { has_admin: true }),
            Arc::new(StubRuntimeDatabaseConnector),
        )
        .await
        .unwrap();

        let response = app_router(mode)
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
}
