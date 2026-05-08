pub mod archive;
pub mod client;
pub mod runner;

use axum::{Json, Router, routing::get};
use grass_worker_config::NodeAppConfig;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

#[derive(Debug)]
pub enum WorkerError {
    Client(client::ClientError),
    Runner(runner::RunnerError),
    Archive(archive::ArchiveError),
    Join(tokio::task::JoinError),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Client(error) => write!(f, "{error}"),
            Self::Runner(error) => write!(f, "{error}"),
            Self::Archive(error) => write!(f, "{error}"),
            Self::Join(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Client(error) => Some(error),
            Self::Runner(error) => Some(error),
            Self::Archive(error) => Some(error),
            Self::Join(error) => Some(error),
        }
    }
}

impl From<client::ClientError> for WorkerError {
    fn from(value: client::ClientError) -> Self {
        Self::Client(value)
    }
}

impl From<runner::RunnerError> for WorkerError {
    fn from(value: runner::RunnerError) -> Self {
        Self::Runner(value)
    }
}

impl From<archive::ArchiveError> for WorkerError {
    fn from(value: archive::ArchiveError) -> Self {
        Self::Archive(value)
    }
}

impl From<tokio::task::JoinError> for WorkerError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::Join(value)
    }
}

async fn root() -> &'static str {
    "grass-worker-node"
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

pub async fn run_worker(config: NodeAppConfig) -> Result<(), WorkerError> {
    let client = client::ControlPlaneClient::new(
        config.node.control_plane_url.clone(),
        config.node.shared_token.clone(),
    )?;
    let runner = runner::DeploymentRunner::new(config.node.work_root.clone());
    let poll_interval = Duration::from_secs(config.node.poll_interval_seconds.max(1));

    loop {
        match client.claim_next_deployment().await? {
            Some(deployment) => {
                process_claimed_deployment(&client, &runner, deployment).await?;
            }
            None => tokio::time::sleep(poll_interval).await,
        }
    }
}

async fn process_claimed_deployment(
    client: &client::ControlPlaneClient,
    runner: &runner::DeploymentRunner,
    deployment: client::ClaimedDeployment,
) -> Result<(), WorkerError> {
    client
        .update_stage(deployment.id, "clone", Some("processing"), None)
        .await?;

    match runner.execute(&deployment).await {
        Ok(output) => {
            client
                .update_stage(deployment.id, "build", Some("processing"), None)
                .await?;

            let deployment_id = deployment.id;
            let bundle = tokio::task::spawn_blocking(move || {
                archive::archive_output_directory_with_id(&output.output_dir, deployment_id)
            })
            .await??;

            client.upload_build_log(deployment.id, &output.log).await?;
            client
                .update_stage(deployment.id, "upload", Some("processing"), None)
                .await?;
            client
                .upload_static_site(deployment.id, &bundle.file_name, bundle.bytes)
                .await?;
        }
        Err(error) => {
            let failure_log = match &error {
                runner::RunnerError::Command { log, .. } => log.clone(),
                _ => format!("{error}\n"),
            };
            let stage = match &error {
                runner::RunnerError::Command { stage, .. } => *stage,
                runner::RunnerError::Validation(_) => "prepare",
                runner::RunnerError::Io(_) => "build",
            };

            client.upload_build_log(deployment.id, &failure_log).await?;
            client
                .update_stage(deployment.id, stage, Some("failed"), Some(&error.to_string()))
                .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use grass_worker_config::{NodeAppConfig, NodeConfig};
    use std::io::Read;
    use tempfile::tempdir;
    use tower::ServiceExt;
    use uuid::Uuid;
    use zip::ZipArchive;

    #[tokio::test]
    async fn root_returns_node_service_name() {
        let response = app_router()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();

        assert_eq!(std::str::from_utf8(&body).unwrap(), "grass-worker-node");
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

    #[test]
    fn worker_executes_git_backed_deployment_in_stage_order() {
        let work_root = tempdir().unwrap().keep();
        let runner = crate::runner::DeploymentRunner::new(work_root);
        let deployment = crate::client::ClaimedDeployment {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            status: "processing".to_owned(),
            source_branch: "main".to_owned(),
            source_revision: Some("deadbeef".to_owned()),
            last_stage: None,
            failure_message: None,
            repository_url: "https://github.com/acme/docs-site".to_owned(),
            production_branch: "main".to_owned(),
            root_directory: Some("apps/docs".to_owned()),
            install_command: "bun install --frozen-lockfile".to_owned(),
            build_command: "bun run build".to_owned(),
            output_directory: "dist".to_owned(),
            started_at: None,
            finished_at: None,
        };

        let plan = runner.build_plan(&deployment).unwrap();

        assert_eq!(plan.source_branch, "main");
        assert_eq!(plan.repository_url, "https://github.com/acme/docs-site");
        assert_eq!(plan.project_root, plan.workspace_dir.join("repo").join("apps/docs"));
        assert_eq!(plan.build_command, "bun run build");
        assert_eq!(plan.output_dir, plan.project_root.join("dist"));
    }

    #[test]
    fn archive_output_directory_creates_zip_with_root_index_html() {
        let root = tempdir().unwrap();
        let output_dir = root.path().join("dist");
        std::fs::create_dir_all(output_dir.join("assets")).unwrap();
        std::fs::write(output_dir.join("index.html"), "<h1>docs</h1>").unwrap();
        std::fs::write(output_dir.join("assets/app.js"), "console.log('ok');").unwrap();

        let bundle = crate::archive::archive_output_directory(&output_dir).unwrap();

        assert_eq!(
            bundle.path.extension().and_then(|value| value.to_str()),
            Some("zip")
        );

        let file = std::fs::File::open(&bundle.path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let mut index_html = String::new();
        archive
            .by_name("index.html")
            .unwrap()
            .read_to_string(&mut index_html)
            .unwrap();
        assert_eq!(index_html, "<h1>docs</h1>");

        let mut app_js = String::new();
        archive
            .by_name("assets/app.js")
            .unwrap()
            .read_to_string(&mut app_js)
            .unwrap();
        assert_eq!(app_js, "console.log('ok');");
    }

    #[test]
    fn worker_config_uses_node_poll_settings() {
        let config = NodeAppConfig {
            node: NodeConfig {
                listen: "127.0.0.1:3001".parse().unwrap(),
                control_plane_url: "http://127.0.0.1:3000".to_owned(),
                shared_token: "node-secret".to_owned(),
                poll_interval_seconds: 2,
                work_root: tempdir().unwrap().keep(),
            },
        };

        assert_eq!(config.node.poll_interval_seconds, 2);
        assert_eq!(config.node.shared_token, "node-secret");
    }
}
