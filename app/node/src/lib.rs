use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

async fn root() -> &'static str {
    "Hello, World from node-agent"
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        service: "node-agent",
        status: "ok",
    })
}

pub fn app_router() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn root_returns_hello_world() {
        let response = app_router()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            "Hello, World from node-agent"
        );
    }

    #[tokio::test]
    async fn health_returns_service_status() {
        let response = app_router()
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

        assert_eq!(json["service"], "node-agent");
        assert_eq!(json["status"], "ok");
    }
}
