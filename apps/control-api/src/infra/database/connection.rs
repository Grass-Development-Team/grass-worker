use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};

pub async fn connect(database_url: &str) -> anyhow::Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(database_url.to_owned());
    options
        .max_connections(10)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);

    Database::connect(options)
        .await
        .map_err(|error| anyhow::anyhow!("failed to connect to PostgreSQL: {error}"))
}
