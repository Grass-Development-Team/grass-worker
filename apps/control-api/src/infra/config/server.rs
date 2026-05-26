use std::net::IpAddr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: IpAddr,
    #[serde(default = "default_api_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_api_port(),
        }
    }
}

fn default_host() -> IpAddr {
    IpAddr::from([127, 0, 0, 1])
}

const fn default_api_port() -> u16 {
    8080
}
