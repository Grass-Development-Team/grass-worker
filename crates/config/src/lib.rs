use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub host: IpAddr,
    pub port: u16,
}

impl ServiceConfig {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }

    fn from_map(
        map: &HashMap<String, String>,
        host_key: &str,
        port_key: &str,
        default_port: u16,
    ) -> Self {
        let host = map
            .get(host_key)
            .and_then(|value| value.parse::<IpAddr>().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let port = map
            .get(port_key)
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_port);

        Self { host, port }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontendConfig {
    pub api_base_url: String,
    pub node_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub api: ServiceConfig,
    pub node: ServiceConfig,
    pub frontend: FrontendConfig,
}

impl AppConfig {
    pub fn defaults() -> Self {
        let vars = HashMap::new();

        Self::from_map(&vars)
    }

    pub fn from_env() -> Self {
        let vars = std::env::vars().collect::<HashMap<String, String>>();

        Self::from_map(&vars)
    }

    fn from_map(vars: &HashMap<String, String>) -> Self {
        let api = ServiceConfig::from_map(vars, "GRASS_API_HOST", "GRASS_API_PORT", 3000);
        let node = ServiceConfig::from_map(vars, "GRASS_NODE_HOST", "GRASS_NODE_PORT", 3001);
        let frontend = FrontendConfig {
            api_base_url: vars
                .get("GRASS_FRONTEND_API_BASE_URL")
                .cloned()
                .unwrap_or_else(|| "http://127.0.0.1:3000".to_owned()),
            node_base_url: vars
                .get("GRASS_FRONTEND_NODE_BASE_URL")
                .cloned()
                .unwrap_or_else(|| "http://127.0.0.1:3001".to_owned()),
        };

        Self {
            api,
            node,
            frontend,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_expected_ports_and_urls() {
        let config = AppConfig::defaults();

        assert_eq!(config.api.socket_addr().to_string(), "127.0.0.1:3000");
        assert_eq!(config.node.socket_addr().to_string(), "127.0.0.1:3001");
        assert_eq!(config.frontend.api_base_url, "http://127.0.0.1:3000");
        assert_eq!(config.frontend.node_base_url, "http://127.0.0.1:3001");
    }

    #[test]
    fn env_values_override_defaults() {
        let vars = HashMap::from([
            ("GRASS_API_HOST".to_owned(), "0.0.0.0".to_owned()),
            ("GRASS_API_PORT".to_owned(), "7000".to_owned()),
            (
                "GRASS_FRONTEND_API_BASE_URL".to_owned(),
                "https://api.example.test".to_owned(),
            ),
        ]);

        let config = AppConfig::from_map(&vars);

        assert_eq!(config.api.socket_addr().to_string(), "0.0.0.0:7000");
        assert_eq!(config.node.socket_addr().to_string(), "127.0.0.1:3001");
        assert_eq!(config.frontend.api_base_url, "https://api.example.test");
    }
}
