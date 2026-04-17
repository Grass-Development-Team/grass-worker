use grass_worker_api::{AppMode, StartupMode, app_router};
use grass_worker_config::ResolvedApiConfig;
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
    match StartupMode::resolve(ResolvedApiConfig::load()?).await? {
        StartupMode::Ready(config) => {
            let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
            let app = app_router(AppMode::Normal(config))?;

            axum::serve(listener, app).await?;
        }
        StartupMode::Setup(context) => {
            let listener = tokio::net::TcpListener::bind(context.listen()).await?;
            let app = app_router(AppMode::Setup(context))?;

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
