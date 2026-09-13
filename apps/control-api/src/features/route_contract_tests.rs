//! Existing public and Node method/path contracts, captured before route slicing.
use axum::{
    body::Body,
    extract::{MatchedPath, Request},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use serde::Deserialize;
use tower::ServiceExt;

use crate::state::ControlApiState;

#[derive(Deserialize)]
struct Contract {
    method: String,
    path: String,
}

async fn mark_matched_path(request: Request, next: Next) -> Response {
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned());
    let mut response = next.run(request).await;
    if let Some(path) = path {
        response
            .headers_mut()
            .insert("x-test-route", HeaderValue::from_str(&path).unwrap());
    }
    response
}

#[tokio::test]
async fn application_preserves_registered_paths_and_http_methods() {
    let contracts: Vec<Contract> =
        serde_json::from_str(include_str!("../../tests/fixtures/routes.json")).unwrap();
    let state = ControlApiState::new(Default::default(), "unused-route-test.toml");
    let app = super::router::router(state.clone())
        // Replace only the default method fallback after production middleware is
        // assembled, so an auth/setup rejection cannot hide a missing method.
        .method_not_allowed_fallback(|| async {
            (StatusCode::METHOD_NOT_ALLOWED, [("x-test-missing-method", "true")])
        })
        .route_layer(middleware::from_fn(mark_matched_path))
        .with_state(state);
    let paths: std::collections::BTreeSet<_> = contracts
        .iter()
        .map(|contract| contract.path.as_str())
        .collect();
    for path in paths {
        let uri = path
            .split('/')
            .map(|segment| {
                if segment.starts_with('{') {
                    "00000000-0000-0000-0000-000000000001"
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        for method in [
            "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE",
        ] {
            let expected = contracts.iter().any(|contract| {
                contract.path == path
                    && (contract.method == method || (method == "HEAD" && contract.method == "GET"))
            });
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(&uri)
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response
                    .headers()
                    .get("x-test-route")
                    .and_then(|value| value.to_str().ok()),
                Some(path),
                "missing path: {method} {path}"
            );
            let registered = !response.headers().contains_key("x-test-missing-method");
            assert_eq!(registered, expected, "changed method: {method} {path}");
        }
    }
}
