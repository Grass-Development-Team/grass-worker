mod frontend;

use axum::routing::any;
use axum::{Json, Router, routing::get};
use frontend::{FrontendMode, install_frontend};
use grass_worker_config::AppConfig;
use serde::Serialize;
use std::sync::OnceLock;

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

pub fn app_router(config: AppConfig) -> std::io::Result<Router> {
    ensure_rustls_crypto_provider();

    let router = Router::new()
        .route("/health", get(health))
        .route("/api/{*path}", any(api_not_found));

    let frontend_mode = match config.development {
        Some(development) => FrontendMode::Development {
            dev_server: development.dev_server,
        },
        None => FrontendMode::Release {
            public_dir: std::env::current_dir()?.join("public"),
        },
    };

    Ok(install_frontend(router, frontend_mode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use grass_worker_config::AppConfig;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_returns_service_status() {
        let response = app_router(AppConfig::defaults())
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
}
