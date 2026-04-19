use grass_worker_config::DatabaseConfig;
use grass_worker_database::connection::{connect, prepare_schema};
use grass_worker_migration::{Migrator, MigratorTrait};
use sea_orm::DatabaseConnection;

pub async fn connect_runtime_database(
    database: &DatabaseConfig,
) -> Result<DatabaseConnection, String> {
    let connection = connect(database).await.map_err(|error| error.to_string())?;
    prepare_schema(&connection, &database.schema)
        .await
        .map_err(|error| error.to_string())?;
    Migrator::up(&connection, None)
        .await
        .map_err(|error| error.to_string())?;

    Ok(connection)
}
