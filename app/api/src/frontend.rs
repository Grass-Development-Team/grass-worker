use axum::{
    Router,
    body::Body,
    extract::Extension,
    http::{HeaderValue, StatusCode, Uri, header::HeaderName},
    response::{IntoResponse, Response},
};
use axum_reverse_proxy::ReverseProxy;
use grass_worker_assets::get_asset;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone)]
pub enum FrontendMode {
    Development { dev_server: String },
    Release { public_dir: PathBuf },
}

#[derive(Debug, Clone)]
struct ReleaseFrontendState {
    public_dir: PathBuf,
}

pub fn install_frontend(router: Router, mode: FrontendMode) -> Router {
    match mode {
        FrontendMode::Development { dev_server } => {
            router.fallback_service(ReverseProxy::new("/", &dev_server))
        }
        FrontendMode::Release { public_dir } => router
            .fallback(release_frontend_handler)
            .layer(Extension(ReleaseFrontendState { public_dir })),
    }
}

async fn release_frontend_handler(
    Extension(state): Extension<ReleaseFrontendState>,
    uri: Uri,
) -> Response {
    match resolve_release_asset(&state.public_dir, uri.path()) {
        Ok(Some(asset)) => asset.into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Debug)]
struct AssetResponse {
    bytes: Vec<u8>,
    content_type: String,
}

impl IntoResponse for AssetResponse {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::from(self.bytes));
        response.headers_mut().insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_str(&self.content_type).unwrap(),
        );

        response
    }
}

fn resolve_release_asset(
    public_dir: &Path,
    request_path: &str,
) -> std::io::Result<Option<AssetResponse>> {
    let requested_path = match normalize_requested_asset_path(request_path) {
        Some(path) => path,
        None => return Ok(None),
    };

    if let Some(asset) = load_public_asset(public_dir, &requested_path)? {
        return Ok(Some(asset));
    }

    if let Some(asset) = load_embedded_asset(&requested_path) {
        return Ok(Some(asset));
    }

    if requested_path != "index.html" && should_use_spa_fallback(request_path) {
        if let Some(asset) = load_public_asset(public_dir, "index.html")? {
            return Ok(Some(asset));
        }

        if let Some(asset) = load_embedded_asset("index.html") {
            return Ok(Some(asset));
        }
    }

    Ok(None)
}

fn load_public_asset(
    public_dir: &Path,
    requested_path: &str,
) -> std::io::Result<Option<AssetResponse>> {
    let asset_path = public_dir.join(requested_path);

    if !asset_path.is_file() {
        return Ok(None);
    }

    let bytes = fs::read(&asset_path)?;

    Ok(Some(asset_response(requested_path, bytes)))
}

fn load_embedded_asset(requested_path: &str) -> Option<AssetResponse> {
    let asset = get_asset(requested_path)?;

    Some(asset_response(requested_path, asset.into_owned()))
}

fn asset_response(requested_path: &str, bytes: Vec<u8>) -> AssetResponse {
    AssetResponse {
        bytes,
        content_type: mime_guess::from_path(requested_path)
            .first_or_octet_stream()
            .to_string(),
    }
}

fn normalize_requested_asset_path(request_path: &str) -> Option<String> {
    let trimmed_path = request_path.trim_start_matches('/');

    if trimmed_path.is_empty() {
        return Some("index.html".to_owned());
    }

    let mut parts = Vec::new();

    for component in Path::new(trimmed_path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }

    if parts.is_empty() {
        return Some("index.html".to_owned());
    }

    let joined = parts.join("/");

    if request_path.ends_with('/') {
        Some(format!("{joined}/index.html"))
    } else {
        Some(joined)
    }
}

fn should_use_spa_fallback(request_path: &str) -> bool {
    let path = request_path.trim_end_matches('/');

    if path.is_empty() {
        return false;
    }

    let last_segment = path.rsplit('/').next().unwrap_or_default();

    !last_segment.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
        response::IntoResponse,
        routing::{any, get},
    };
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn backend_shell() -> Router {
        Router::new()
            .route(
                "/health",
                get(|| async { Json(json!({ "service": "control-api", "status": "ok" })) }),
            )
            .route(
                "/api/{*path}",
                any(|| async { StatusCode::NOT_FOUND.into_response() }),
            )
    }

    #[tokio::test]
    async fn release_mode_prefers_runtime_public_directory() {
        let temp_dir = tempdir().unwrap();
        let public_dir = temp_dir.path().join("public");
        fs::create_dir_all(&public_dir).unwrap();
        fs::write(
            public_dir.join("index.html"),
            "<html>public override</html>",
        )
        .unwrap();

        let app = install_frontend(
            backend_shell(),
            FrontendMode::Release {
                public_dir: public_dir.clone(),
            },
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("public override")
        );
    }

    #[tokio::test]
    async fn release_mode_falls_back_to_embedded_assets() {
        let temp_dir = tempdir().unwrap();
        let app = install_frontend(
            backend_shell(),
            FrontendMode::Release {
                public_dir: temp_dir.path().join("public"),
            },
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(html.contains("grass-worker"));
    }

    #[tokio::test]
    async fn release_mode_uses_spa_fallback_for_non_asset_paths() {
        let temp_dir = tempdir().unwrap();
        let app = install_frontend(
            backend_shell(),
            FrontendMode::Release {
                public_dir: temp_dir.path().join("public"),
            },
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/projects/example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();

        assert!(html.contains("grass-worker"));
    }

    #[tokio::test]
    async fn development_mode_proxies_frontend_requests() {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = upstream.local_addr().unwrap();
        let upstream_app = Router::new().route("/", get(|| async { "frontend dev server" }));
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream, upstream_app).await.unwrap();
        });

        let app = install_frontend(
            backend_shell(),
            FrontendMode::Development {
                dev_server: format!("http://{address}"),
            },
        );

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        upstream_task.abort();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(std::str::from_utf8(&body).unwrap(), "frontend dev server");
    }

    #[tokio::test]
    async fn backend_routes_remain_backend_owned() {
        let temp_dir = tempdir().unwrap();
        let app = install_frontend(
            backend_shell(),
            FrontendMode::Release {
                public_dir: temp_dir.path().join("public"),
            },
        );

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let api = app
            .oneshot(
                Request::builder()
                    .uri("/api/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::NOT_FOUND);
    }
}
