use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
};

use anyhow::Context;
use grass_config::{ConfigError, load_toml_or_default, overlay_string, overlay_u16, overlay_u64};
use serde::Deserialize;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LogConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub format: LogFormat,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: LogFormat::Pretty,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct NodeIdentityConfig {
    #[serde(default = "default_node_id")]
    pub id: String,
    #[serde(default = "default_control_api")]
    pub control_api: String,
    #[serde(default = "default_node_token")]
    pub node_token: String,
    #[serde(default = "default_node_work_root")]
    pub work_root: String,
    #[serde(default)]
    pub capabilities: NodeCapabilitiesConfig,
}

impl Default for NodeIdentityConfig {
    fn default() -> Self {
        Self {
            id: default_node_id(),
            control_api: default_control_api(),
            node_token: default_node_token(),
            work_root: default_node_work_root(),
            capabilities: NodeCapabilitiesConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct NodeCapabilitiesConfig {
    #[serde(default = "default_true")]
    pub build: bool,
    #[serde(default = "default_true")]
    pub serve: bool,
}

impl Default for NodeCapabilitiesConfig {
    fn default() -> Self {
        Self {
            build: true,
            serve: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BuildConfig {
    #[serde(default = "default_build_concurrency")]
    pub concurrency: u16,
    #[serde(default = "default_build_timeout_seconds")]
    pub command_timeout_seconds: u64,
    #[serde(default)]
    pub retain_workspace_on_failure: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            concurrency: default_build_concurrency(),
            command_timeout_seconds: default_build_timeout_seconds(),
            retain_workspace_on_failure: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ServeConfig {
    #[serde(default = "default_serve_host")]
    pub host: IpAddr,
    #[serde(default = "default_serve_port")]
    pub port: u16,
    #[serde(default = "default_serve_public_base_url")]
    pub public_base_url: String,
    #[serde(default = "default_metadata_cache_ttl_seconds")]
    pub metadata_cache_ttl_seconds: u64,
    #[serde(default = "default_artifact_cache_root")]
    pub artifact_cache_root: String,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            host: default_serve_host(),
            port: default_serve_port(),
            public_base_url: default_serve_public_base_url(),
            metadata_cache_ttl_seconds: default_metadata_cache_ttl_seconds(),
            artifact_cache_root: default_artifact_cache_root(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_backend")]
    pub backend: String,
    #[serde(default = "default_runtime_socket")]
    pub socket: String,
    #[serde(default = "default_build_image")]
    pub default_build_image: String,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default)]
    pub resources: RuntimeResourcesConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            backend: default_runtime_backend(),
            socket: default_runtime_socket(),
            default_build_image: default_build_image(),
            network: default_network(),
            resources: RuntimeResourcesConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RuntimeResourcesConfig {
    #[serde(default = "default_cpu_limit")]
    pub cpu_limit: u32,
    #[serde(default = "default_memory_mb")]
    pub memory_mb: u64,
}

impl Default for RuntimeResourcesConfig {
    fn default() -> Self {
        Self {
            cpu_limit: default_cpu_limit(),
            memory_mb: default_memory_mb(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct SecurityConfig {
    #[serde(default)]
    pub allow_private_repository: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub verbose_build_log: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct NodeConfig {
    #[serde(default)]
    pub node: NodeIdentityConfig,
    #[serde(default)]
    pub build: BuildConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub serve: ServeConfig,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub development: DevelopmentConfig,
    #[serde(default)]
    pub log: LogConfig,
}

impl NodeConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let mut config = load_toml_or_default(path)?;
        apply_env(&mut config)?;
        Ok(config)
    }

    pub fn init_tracing(&self) -> anyhow::Result<()> {
        let filter = EnvFilter::try_new(&self.log.level).context("invalid tracing filter")?;
        let subscriber = fmt().with_env_filter(filter);

        match self.log.format {
            LogFormat::Pretty => subscriber
                .try_init()
                .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
            LogFormat::Json => subscriber
                .json()
                .try_init()
                .map_err(|error| anyhow::anyhow!("failed to initialize tracing: {error}"))?,
        }

        Ok(())
    }
}

fn apply_env(config: &mut NodeConfig) -> Result<(), ConfigError> {
    overlay_string("GWNODE_ID", &mut config.node.id);
    overlay_string("GWNODE_CONTROL_API", &mut config.node.control_api);
    overlay_string("GWNODE_NODE_TOKEN", &mut config.node.node_token);
    overlay_string("GWNODE_WORK_ROOT", &mut config.node.work_root);
    overlay_u16("GWNODE_BUILD_CONCURRENCY", &mut config.build.concurrency)?;
    overlay_u64(
        "GWNODE_BUILD_COMMAND_TIMEOUT_SECONDS",
        &mut config.build.command_timeout_seconds,
    )?;
    if let Ok(value) = std::env::var("GWNODE_SERVE_LISTEN") {
        let listen = parse_listen("GWNODE_SERVE_LISTEN", &value)?;
        config.serve.host = listen.ip();
        config.serve.port = listen.port();
    }
    overlay_string(
        "GWNODE_SERVE_PUBLIC_BASE_URL",
        &mut config.serve.public_base_url,
    );
    overlay_string("GWNODE_LOG_LEVEL", &mut config.log.level);
    overlay_string("LOG_LEVEL", &mut config.log.level);
    Ok(())
}

fn parse_listen(name: &'static str, value: &str) -> Result<SocketAddr, ConfigError> {
    value.parse().map_err(|source| ConfigError::Env {
        name,
        source: Box::new(source),
    })
}

fn default_log_level() -> String {
    "info".to_owned()
}

fn default_node_id() -> String {
    "local-node".to_owned()
}

fn default_control_api() -> String {
    "http://127.0.0.1:7817".to_owned()
}

fn default_node_token() -> String {
    "change-me".to_owned()
}

fn default_node_work_root() -> String {
    "/data/node".to_owned()
}

const fn default_true() -> bool {
    true
}

const fn default_build_concurrency() -> u16 {
    1
}

const fn default_build_timeout_seconds() -> u64 {
    600
}

fn default_serve_host() -> IpAddr {
    IpAddr::from([0, 0, 0, 0])
}

const fn default_serve_port() -> u16 {
    8080
}

fn default_serve_public_base_url() -> String {
    "http://127.0.0.1:8080".to_owned()
}

const fn default_metadata_cache_ttl_seconds() -> u64 {
    30
}

fn default_artifact_cache_root() -> String {
    "/data/node/artifacts".to_owned()
}

fn default_runtime_backend() -> String {
    "podman-socket".to_owned()
}

fn default_runtime_socket() -> String {
    // Podman rootless socket for the current user; Docker deployments set
    // unix:///var/run/docker.sock.
    "unix:///run/user/1000/podman/podman.sock".to_owned()
}

fn default_build_image() -> String {
    "docker.io/library/node:22".to_owned()
}

fn default_network() -> String {
    "bridge".to_owned()
}

const fn default_cpu_limit() -> u32 {
    2
}

const fn default_memory_mb() -> u64 {
    2048
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_control_api_uses_control_plane_port() {
        assert_eq!(
            NodeConfig::default().node.control_api,
            "http://127.0.0.1:7817"
        );
    }

    #[test]
    fn serve_listen_uses_socket_address_format() {
        let listen = parse_listen("GWNODE_SERVE_LISTEN", "0.0.0.0:8080").unwrap();
        assert_eq!(listen.ip().to_string(), "0.0.0.0");
        assert_eq!(listen.port(), 8080);
        assert!(parse_listen("GWNODE_SERVE_LISTEN", "localhost").is_err());
    }
}
