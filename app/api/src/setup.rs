use crate::startup::{SetupContext, SetupStage};
use axum::{Extension, Json, Router, response::Html, routing::get};
use serde::Serialize;

const SETUP_PLACEHOLDER_PAGE: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>grass-worker setup</title>
  </head>
  <body>
    <main>
      <h1>grass-worker setup</h1>
      <p>Setup mode is active. Continue configuration via API.</p>
    </main>
  </body>
</html>
"#;

#[derive(Debug, Serialize)]
struct SetupStateResponse {
    stage: SetupStage,
    status: &'static str,
}

pub fn install_setup(router: Router, context: SetupContext) -> Router {
    router
        .route("/", get(setup_page))
        .route("/api/setup/state", get(setup_state))
        .layer(Extension(context))
}

async fn setup_page() -> Html<&'static str> {
    Html(SETUP_PLACEHOLDER_PAGE)
}

async fn setup_state(Extension(context): Extension<SetupContext>) -> Json<SetupStateResponse> {
    Json(SetupStateResponse {
        stage: context.stage,
        status: "pending",
    })
}
