use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use grass_git_source::PrivateTargetException;
use grass_node_protocol::{
    RouteSnapshotResponse, ServeAccess, ServeArtifact, ServeAssignment, ServeAssignmentStatus,
    ServeResources, ServeRoute,
};
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::{
    build::git::{CheckoutAccess, checkout},
    client::ControlApiClient,
    config::NodeConfig,
    output::generate_grass_output,
    runtime::{ContainerRuntime, PrepareImageInput, RunBuildInput, SocketRuntime},
    serve::{ServeState, routes::RouteTable, ssr::SsrManager, sync::stage_archive},
};

use super::serve_router;

const FIXTURE_HOST: &str = "release-smoke.grass.test";

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "grass-node-release-smoke-{}",
            Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ServerTask(tokio::task::JoinHandle<()>);

impl Drop for ServerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn create_vite_repository(root: &Path) -> anyhow::Result<(PathBuf, String)> {
    let source = root.join("fixture-source");
    std::fs::create_dir_all(source.join("src"))?;
    std::fs::write(
        source.join("package.json"),
        r#"{
  "name": "grass-node-release-smoke",
  "private": true,
  "version": "1.0.0",
  "type": "module",
  "scripts": { "build": "vite build" },
  "devDependencies": { "vite": "8.1.3" }
}
"#,
    )?;
    std::fs::write(
        source.join("index.html"),
        r#"<!doctype html>
<html lang="en">
  <head><meta charset="UTF-8"><title>Grass Worker delivery smoke</title></head>
  <body><div id="app">Grass Worker delivery smoke</div><script type="module" src="/src/main.js"></script></body>
</html>
"#,
    )?;
    std::fs::write(
        source.join("src/main.js"),
        "document.querySelector('#app').textContent = 'Delivered by grass-worker';\n",
    )?;
    std::fs::write(
        source.join("vite.config.js"),
        r#"import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rollupOptions: {
      output: {
        entryFileNames: "assets/app.js",
        chunkFileNames: "assets/[name].js",
        assetFileNames: "assets/[name][extname]"
      }
    }
  }
});
"#,
    )?;

    run_git(&source, &["init", "--initial-branch=main"])?;
    run_git(&source, &["config", "user.name", "Grass Worker CI"])?;
    run_git(
        &source,
        &["config", "user.email", "grass-worker-ci@example.invalid"],
    )?;
    run_git(&source, &["add", "."])?;
    run_git(&source, &["commit", "-m", "Create Vite smoke fixture"])?;
    let commit = run_git(&source, &["rev-parse", "HEAD"])?;

    let served = root.join("served");
    std::fs::create_dir(&served)?;
    let bare = served.join("repository.git");
    let source_arg = source.to_string_lossy().into_owned();
    let bare_arg = bare.to_string_lossy().into_owned();
    run_git(root, &["clone", "--bare", &source_arg, &bare_arg])?;
    run_git(&bare, &["update-server-info"])?;
    Ok((served, commit))
}

fn safe_repository_path(root: &Path, request_path: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for component in Path::new(request_path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(segment) => path.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

async fn repository_file(State(root): State<Arc<PathBuf>>, request: Request) -> Response {
    let Some(path) = safe_repository_path(&root, request.uri().path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let content_type = if request.uri().path().ends_with("/info/refs") {
                "text/plain; charset=utf-8"
            } else {
                "application/octet-stream"
            };
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(bytes))
                .expect("static response headers are valid")
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn spawn_repository_server(root: PathBuf) -> anyhow::Result<(SocketAddr, ServerTask)> {
    let app = Router::new()
        .fallback(get(repository_file))
        .with_state(Arc::new(root));
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app.into_make_service()).await;
    });
    Ok((address, ServerTask(task)))
}

async fn spawn_delivery_server(app: Router) -> anyhow::Result<(SocketAddr, ServerTask)> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((address, ServerTask(task)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker and the grass build image"]
async fn checks_out_builds_packages_stages_and_serves_vite() -> anyhow::Result<()> {
    let image = std::env::var("GRASS_NODE_SMOKE_IMAGE")
        .context("GRASS_NODE_SMOKE_IMAGE must name the prebuilt smoke image")?;
    let socket = std::env::var("GRASS_NODE_SMOKE_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
    let network = std::env::var("GRASS_NODE_SMOKE_NETWORK").unwrap_or_else(|_| "bridge".to_owned());

    let root = TestRoot::new()?;
    let (repository_root, commit) = create_vite_repository(&root.0)?;
    let (repository_address, _repository_server) = spawn_repository_server(repository_root).await?;
    let repository_url = format!(
        "http://127.0.0.1:{}/repository.git",
        repository_address.port()
    );
    let exceptions = [PrivateTargetException {
        host: "127.0.0.1".to_owned(),
        ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: repository_address.port(),
    }];
    let workspace = root.0.join("workspace");
    let checked_out = checkout(
        &repository_url,
        None,
        Some(&commit),
        None,
        &workspace,
        CheckoutAccess {
            private_target_exceptions: &exceptions,
            credential: None,
            known_hosts_line: None,
            ssh_target_ip: None,
        },
    )
    .await
    .context("production checkout failed")?;
    anyhow::ensure!(
        checked_out.commit_hash.as_deref() == Some(commit.as_str()),
        "checkout resolved an unexpected commit"
    );

    let runtime = SocketRuntime::connect("docker-socket", &socket)?;
    let (log_sender, mut log_receiver) = mpsc::channel(4096);
    runtime
        .prepare_image(PrepareImageInput { image: &image }, log_sender.clone())
        .await
        .context("build image is unavailable")?;
    let (_cancel_sender, cancel_receiver) = watch::channel(false);
    let result = runtime
        .run_build(
            RunBuildInput {
                image,
                workspace: checked_out.source_dir.clone(),
                working_dir: ".".to_owned(),
                script: "npm install --no-audit --no-fund --no-update-notifier && npm run build"
                    .to_owned(),
                env: vec![("CI".to_owned(), "true".to_owned())],
                cpu_limit: 1,
                memory_mb: 1024,
                network,
                timeout: Some(Duration::from_secs(300)),
                export_paths: vec!["dist".to_owned()],
            },
            log_sender,
            cancel_receiver,
        )
        .await
        .context("production container build failed")?;
    let mut build_logs = Vec::new();
    while let Ok(line) = log_receiver.try_recv() {
        build_logs.push(line);
    }
    anyhow::ensure!(
        result.exit_code == 0,
        "Vite build exited with {}:\n{}",
        result.exit_code,
        build_logs.join("\n")
    );
    anyhow::ensure!(
        checked_out.project_root.join("dist/index.html").is_file(),
        "container runtime did not export the Vite dist directory"
    );

    let generated = generate_grass_output(
        &checked_out.project_root,
        Some("dist"),
        Some("npm run build"),
    )
    .context("Grass Output generation failed")?;
    anyhow::ensure!(generated.runtime_kind == "static");
    anyhow::ensure!(generated.framework_name == "vite");
    let archive_path = root.0.join("grass-output.zip");
    let packed = grass_archive::pack_dir(&generated.output_root, &archive_path)?;

    let node_id = Uuid::now_v7();
    let deployment_id = Uuid::now_v7();
    let resources = ServeResources {
        cpu_millicores: 100,
        memory_mb: 128,
        disk_mb: 64,
    };
    let assignment = ServeAssignment {
        deployment_id,
        project_id: Uuid::now_v7(),
        runtime_kind: generated.runtime_kind.to_owned(),
        status: ServeAssignmentStatus::Ready,
        artifact: ServeArtifact {
            artifact_id: Uuid::now_v7(),
            checksum_sha256: packed.checksum_sha256,
            packed_size_bytes: packed.size_bytes,
            unpacked_size_bytes: packed.unpacked_size_bytes,
        },
        resources,
    };
    let cache_root = root.0.join("artifact-cache");
    let staged = stage_archive(&cache_root, &assignment, &archive_path)
        .context("production artifact staging failed")?;
    anyhow::ensure!(staged.join("output.toml").is_file());

    let routes = Arc::new(RouteTable::default());
    routes
        .apply(RouteSnapshotResponse {
            revision: "release-smoke-v1".to_owned(),
            routes: vec![ServeRoute {
                host: FIXTURE_HOST.to_owned(),
                deployment_id,
                target_node_id: node_id,
                target_base_url: "http://127.0.0.1:1".to_owned(),
                resources,
                access: ServeAccess::Public,
            }],
        })
        .await?;
    let mut config = NodeConfig::default();
    config.serve.artifact_cache_root = cache_root.to_string_lossy().into_owned();
    let client = ControlApiClient::new("http://127.0.0.1:1", "release-smoke-token")?;
    let ssr = Arc::new(SsrManager::new(None, node_id, &config));
    let state = Arc::new(ServeState::new(
        client,
        node_id,
        "release-smoke-gateway-token".to_owned(),
        routes,
        &config,
        ssr,
    ));
    let (delivery_address, _delivery_server) = spawn_delivery_server(serve_router(state)).await?;
    let http = reqwest::Client::new();

    let index = http
        .get(format!("http://{delivery_address}/"))
        .header(header::HOST, FIXTURE_HOST)
        .send()
        .await?;
    anyhow::ensure!(index.status() == StatusCode::OK);
    anyhow::ensure!(
        index
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"))
    );
    let index_body = index.text().await?;
    anyhow::ensure!(index_body.contains("Grass Worker delivery smoke"));
    anyhow::ensure!(index_body.contains("/assets/app.js"));

    let asset = http
        .get(format!("http://{delivery_address}/assets/app.js"))
        .header(header::HOST, FIXTURE_HOST)
        .send()
        .await?;
    anyhow::ensure!(asset.status() == StatusCode::OK);
    let asset_body = asset.text().await?;
    anyhow::ensure!(asset_body.contains("Delivered by grass-worker"));

    Ok(())
}
