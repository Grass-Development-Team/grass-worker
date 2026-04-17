use grass_worker_api::{AppMode, StartupMode, app_router};
use grass_worker_config::ResolvedApiConfig;
use grass_worker_database::connection::{connect, prepare_schema};
use grass_worker_migration::{Migrator, MigratorTrait};
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
    match StartupMode::from_api_config(ResolvedApiConfig::load()?) {
        StartupMode::Ready(config) => {
            let database_config = config
                .database
                .as_ref()
                .expect("ready mode requires database config");
            let database = connect(database_config).await?;
            prepare_schema(&database, &database_config.schema).await?;
            Migrator::up(&database, None).await?;
            let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
            let app = app_router(AppMode::Normal(config))?;
            let _database = database;

            axum::serve(listener, app).await?;
        }
        StartupMode::Setup(context) => {
            let listener = tokio::net::TcpListener::bind(context.listen).await?;
            let app = app_router(AppMode::Setup(context))?;

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
