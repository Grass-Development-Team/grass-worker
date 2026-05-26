use std::path::Path;

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

const PUBLIC_DIR: &str = "./public";

/// Serves frontend assets: `./public` override → embedded → SPA fallback.
pub async fn frontend_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // Reject path traversal attempts.
    if path.contains("..") || path.contains('\\') {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // Try ./public override first.
    let public_path = Path::new(PUBLIC_DIR).join(path);
    if public_path.is_file() {
        if let Ok(content) = tokio::fs::read(&public_path).await {
            let mime = mime_guess::from_path(&public_path).first_or_octet_stream();
            return Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .header(header::CACHE_CONTROL, "public, max-age=0")
                .body(Body::from(content))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
    }

    // Try embedded asset.
    if let Some(asset) = grass_assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let cache_control = if has_asset_hash(path) {
            "public, max-age=31536000, immutable"
        } else {
            "public, max-age=0"
        };
        return Response::builder()
            .header(header::CONTENT_TYPE, mime.as_ref())
            .header(header::CACHE_CONTROL, cache_control)
            .body(Body::from(asset.data))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    // SPA fallback: serve index.html for non-asset routes.
    if let Some(asset) = grass_assets::get("index.html") {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "public, max-age=0")
            .body(Body::from(asset.data))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    StatusCode::NOT_FOUND.into_response()
}

/// Returns true if the path contains a hashed asset filename (e.g. `index-aU4XWQwB.js`).
fn has_asset_hash(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .and_then(|name| {
            name.strip_suffix(".js")
                .or_else(|| name.strip_suffix(".css"))
        })
        .is_some_and(|base| base.rsplit('-').next().is_some_and(|h| h.len() >= 6))
}
