//! Resolve staged deployments and deliver their static or SSR output.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::Request,
    http::{StatusCode, header},
    response::Response,
};
use grass_node_protocol::{ServeAccess, ServeRoute};
use tracing::warn;
use uuid::Uuid;

use super::{
    ServeState, normalize_public_path,
    paths::resolve_not_found_file,
    preview::{
        callback_code, handle_preview_callback, is_preview_callback, preview_cookie_value,
        request_destination, require_preview_access, strip_preview_cookie_header,
    },
    proxy::forward_to_ssr,
    resolve_static_file,
    response::error_page,
    routing::{GATEWAY_HOP_HEADER, GATEWAY_TOKEN_HEADER, GatewayOrigin},
    static_files, sync,
};
use crate::output::manifest;

#[derive(Clone)]
pub(super) enum ResolvedTarget {
    Static {
        static_dir: PathBuf,
        spa_fallback: bool,
        not_found: Option<String>,
    },
    Ssr {
        deployment_id: Uuid,
        deployment_dir: PathBuf,
        server: manifest::ServerSection,
    },
}

pub(super) async fn serve_local(
    state: Arc<ServeState>,
    route: ServeRoute,
    client_addr: SocketAddr,
    origin: GatewayOrigin,
    mut request: Request,
) -> Response {
    request.headers_mut().remove(GATEWAY_TOKEN_HEADER);
    request.headers_mut().remove(GATEWAY_HOP_HEADER);

    let target = match resolve_deployment(&state, route.deployment_id).await {
        Ok(target) => target,
        Err(error) => {
            warn!(operation = "node.serve.resolve_host", %error, host = %route.host, "local deployment resolution failed");
            return error_page(
                StatusCode::BAD_GATEWAY,
                "The assigned deployment is not ready on this Serve Node.",
            );
        }
    };

    let requires_preview_access = matches!(route.access, ServeAccess::TeamOrPlatformAdmin);
    if requires_preview_access {
        if is_preview_callback(request.uri().path()) {
            let code = callback_code(&request);
            return handle_preview_callback(&state, &route.host, code).await;
        }
        let destination = request_destination(
            request
                .uri()
                .path_and_query()
                .map(|value| value.as_str())
                .unwrap_or("/"),
        );
        let grant = request
            .headers()
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .and_then(preview_cookie_value)
            .map(str::to_owned);
        if let Err(response) = require_preview_access(&state, &route.host, destination, grant).await
        {
            return response;
        }
    }

    match target {
        ResolvedTarget::Static {
            static_dir,
            spa_fallback,
            not_found,
        } => {
            let method = request.method().clone();
            let range = request.headers().get(header::RANGE).cloned();
            let Some(segments) = normalize_public_path(request.uri().path()) else {
                return error_page(
                    StatusCode::BAD_REQUEST,
                    "The requested path is not allowed.",
                );
            };

            match resolve_static_file(&static_dir, &segments, spa_fallback) {
                Some(file) => serve_file(&file, StatusCode::OK, &method, range.as_ref()).await,
                None => {
                    if let Some(not_found_file) =
                        resolve_not_found_file(&static_dir, not_found.as_deref())
                    {
                        return serve_file(
                            &not_found_file,
                            StatusCode::NOT_FOUND,
                            &method,
                            range.as_ref(),
                        )
                        .await;
                    }
                    error_page(StatusCode::NOT_FOUND, "This page could not be found.")
                }
            }
        }
        ResolvedTarget::Ssr {
            deployment_id,
            deployment_dir,
            server,
        } => {
            if requires_preview_access {
                strip_preview_cookie_header(request.headers_mut());
            }
            let upstream = match state
                .ssr
                .upstream_for(deployment_id, &deployment_dir, &server, route.resources)
                .await
            {
                Ok(upstream) => upstream,
                Err(error) => {
                    warn!(
                        operation = "node.serve.ssr_start",
                        %error,
                        deployment_id = %deployment_id,
                        "ssr service unavailable"
                    );
                    return error_page(
                        StatusCode::BAD_GATEWAY,
                        "The application server failed to start.",
                    );
                }
            };
            match forward_to_ssr(&state.proxy, &upstream, client_addr, origin, request).await {
                Ok(response) => response,
                Err(error) => {
                    // A connect failure means the container died or lost its
                    // address; drop it so the next request restarts it.
                    if error.is_connect() {
                        state.ssr.invalidate(deployment_id).await;
                    }
                    warn!(
                        operation = "node.serve.ssr_proxy",
                        %error,
                        deployment_id = %deployment_id,
                        "ssr proxy request failed"
                    );
                    error_page(
                        StatusCode::BAD_GATEWAY,
                        "The application server could not be reached.",
                    )
                }
            }
        }
    }
}

async fn serve_file(
    path: &Path,
    status: StatusCode,
    method: &axum::http::Method,
    range: Option<&axum::http::HeaderValue>,
) -> Response {
    match static_files::serve_file(path, method, range, status).await {
        Ok(response) => response,
        Err(_) => error_page(StatusCode::NOT_FOUND, "This page could not be found."),
    }
}

async fn resolve_deployment(
    state: &ServeState,
    deployment_id: Uuid,
) -> anyhow::Result<ResolvedTarget> {
    if let Some(target) = state.targets.lock().await.get(&deployment_id).cloned() {
        return Ok(target);
    }
    let target = ensure_artifact(state, deployment_id).await?;
    state
        .targets
        .lock()
        .await
        .insert(deployment_id, target.clone());
    Ok(target)
}

/// Loads the already staged deployment artifact and returns the serve target
/// described by its manifest.
async fn ensure_artifact(
    state: &ServeState,
    deployment_id: Uuid,
) -> anyhow::Result<ResolvedTarget> {
    let deployment_dir = sync::staged_artifact_path(&state.cache_root, deployment_id)?;
    let manifest_path = deployment_dir.join("output.toml");

    let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
    let manifest = manifest::parse_manifest(&manifest_content)
        .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;
    manifest::validate_manifest(&manifest, &deployment_dir)
        .map_err(|error| anyhow::anyhow!("invalid output manifest: {error}"))?;

    if manifest.runtime.kind == "ssr" {
        let server = manifest
            .server
            .ok_or_else(|| anyhow::anyhow!("ssr manifest has no server section"))?;
        return Ok(ResolvedTarget::Ssr {
            deployment_id,
            deployment_dir,
            server,
        });
    }

    let static_section = manifest
        .static_site
        .ok_or_else(|| anyhow::anyhow!("manifest has no static section"))?;

    Ok(ResolvedTarget::Static {
        static_dir: deployment_dir.join(static_section.directory),
        spa_fallback: static_section.spa_fallback,
        not_found: (!static_section.not_found.trim().is_empty())
            .then(|| static_section.not_found.trim().to_owned()),
    })
}
