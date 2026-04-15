use grass_config::AppConfig;
use grass_node::app_router;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let config = AppConfig::from_env();
    let listener = tokio::net::TcpListener::bind(config.node.socket_addr()).await?;

    axum::serve(listener, app_router()).await
}
