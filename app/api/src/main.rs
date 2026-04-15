use grass_api::app_router;
use grass_config::AppConfig;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig::from_env();
    let listener = tokio::net::TcpListener::bind(config.api.socket_addr()).await?;

    axum::serve(listener, app_router()).await
}
