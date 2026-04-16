use grass_worker_api::app_router;
use grass_worker_config::AppConfig;
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
    let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
    let app = app_router(config)?;

    axum::serve(listener, app).await?;
    Ok(())
}
