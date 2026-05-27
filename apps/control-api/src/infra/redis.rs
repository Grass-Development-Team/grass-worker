use anyhow::Context;
use redis::aio::MultiplexedConnection;
use tracing::info;

pub async fn connect(url: &str) -> anyhow::Result<MultiplexedConnection> {
    let client = redis::Client::open(url).context("failed to create Redis client from URL")?;
    let conn = client
        .get_multiplexed_async_connection()
        .await
        .context("failed to establish Redis multiplexed connection")?;
    info!(
        operation = "control_api.redis_connected",
        "Redis connected successfully"
    );
    Ok(conn)
}
