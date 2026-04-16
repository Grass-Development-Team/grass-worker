use grass_worker_config::AppConfig;
use grass_worker_node::app_router;
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
    let config = AppConfig::load()?;
    let listener = tokio::net::TcpListener::bind(config.node.listen).await?;

    axum::serve(listener, app_router()).await?;
    Ok(())
}
