use grass_worker_config::NodeAppConfig;
use grass_worker_node::{app_router, run_worker};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = NodeAppConfig::load()?;
    let listener = tokio::net::TcpListener::bind(config.node.listen).await?;
    let worker_config = config.clone();

    tokio::try_join!(
        async move {
            axum::serve(listener, app_router()).await?;
            Ok::<(), Box<dyn std::error::Error>>(())
        },
        async move {
            run_worker(worker_config).await?;
            Ok::<(), Box<dyn std::error::Error>>(())
        }
    )?;

    Ok(())
}
