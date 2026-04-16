use grass_worker_config::DatabaseConfig;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, Statement};

pub fn postgres_connection_string(config: &DatabaseConfig) -> String {
    format!(
        "postgres://{}:{}@{}:{}/{}",
        config.user, config.password, config.host, config.port, config.db_name
    )
}

pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    Database::connect(postgres_connection_string(config)).await
}

pub async fn prepare_schema(connection: &DatabaseConnection, schema: &str) -> Result<(), DbErr> {
    if connection.get_database_backend() != DbBackend::Postgres {
        return Err(DbErr::Custom(
            "grass-worker database layer only supports PostgreSQL".to_owned(),
        ));
    }

    connection
        .execute(Statement::from_string(
            DbBackend::Postgres,
            create_schema_sql(schema)?,
        ))
        .await?;
    connection
        .execute(Statement::from_string(
            DbBackend::Postgres,
            set_search_path_sql(schema)?,
        ))
        .await?;

    Ok(())
}

pub fn create_schema_sql(schema: &str) -> Result<String, DbErr> {
    validate_schema_name(schema)?;
    Ok(format!(r#"CREATE SCHEMA IF NOT EXISTS "{schema}""#))
}

pub fn set_search_path_sql(schema: &str) -> Result<String, DbErr> {
    validate_schema_name(schema)?;
    Ok(format!(r#"SET search_path TO "{schema}""#))
}

fn validate_schema_name(schema: &str) -> Result<(), DbErr> {
    let mut chars = schema.chars();

    match chars.next() {
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => {
            return Err(DbErr::Custom(
                "invalid postgres schema name; use letters, digits, and underscores only"
                    .to_owned(),
            ));
        }
    }

    if chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        Ok(())
    } else {
        Err(DbErr::Custom(
            "invalid postgres schema name; use letters, digits, and underscores only"
                .to_owned(),
        ))
    }
}
