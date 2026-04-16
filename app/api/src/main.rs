use grass_worker_api::app_router;
use grass_worker_config::AppConfig;
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
    let config = AppConfig::load()?;
    let database = connect(&config.database).await?;
    prepare_schema(&database, &config.database.schema).await?;
    Migrator::up(&database, None).await?;
    let listener = tokio::net::TcpListener::bind(config.server.listen).await?;
    let app = app_router(config)?;
    let _database = database;

    axum::serve(listener, app).await?;
    Ok(())
}
