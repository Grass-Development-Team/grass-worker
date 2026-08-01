use std::{
    net::{IpAddr, SocketAddr},
    path::Path,
};

use anyhow::Context;
use grass_config::{
    ConfigError, load_toml_or_default, overlay_string, overlay_u16, overlay_u64, save_toml,
};
use grass_git_source::PrivateTargetException;
use grass_node_protocol::{
    NodeBuildConfiguration, NodeCapabilities, NodeConfiguration, NodeDevelopmentConfiguration,
    NodeIdentityConfiguration, NodeLogConfiguration, NodeLogFormat,
    NodePrivateRepositoryTargetConfiguration, NodeRuntimeConfiguration,
    NodeRuntimeResourcesConfiguration, NodeSecurityConfiguration, NodeServeCapacityConfiguration,
    NodeServeConfiguration, NodeSsrConfiguration,
};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
    Pretty,
    Json,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    #[serde(default)]
    pub capacity: ServeCapacityConfig,
    #[serde(default)]
    pub ssr: SsrServeConfig,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            host: default_serve_host(),
            port: default_serve_port(),
            public_base_url: default_serve_public_base_url(),
            metadata_cache_ttl_seconds: default_metadata_cache_ttl_seconds(),
            artifact_cache_root: default_artifact_cache_root(),
            capacity: ServeCapacityConfig::default(),
            ssr: SsrServeConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServeCapacityConfig {
    #[serde(default)]
    pub cpu_millicores: u64,
    #[serde(default)]
    pub memory_mb: u64,
    #[serde(default)]
    pub disk_mb: u64,
    #[serde(default = "default_max_deployments")]
    pub max_deployments: u32,
}

impl Default for ServeCapacityConfig {
    fn default() -> Self {
        Self {
            cpu_millicores: 0,
            memory_mb: 0,
            disk_mb: 0,
            max_deployments: default_max_deployments(),
        }
    }
}

/// SSR service lifecycle settings. Services start on the first request for
/// their deployment and stop again after sitting idle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SsrServeConfig {
    /// Idle seconds before a service container is stopped; 0 keeps
    /// services running until the deployment stops resolving.
    #[serde(default = "default_ssr_idle_stop_seconds")]
    pub idle_stop_seconds: u64,
    /// Seconds to wait for a freshly started service to accept connections.
    #[serde(default = "default_ssr_startup_timeout_seconds")]
    pub startup_timeout_seconds: u64,
}

impl Default for SsrServeConfig {
    fn default() -> Self {
        Self {
            idle_stop_seconds: default_ssr_idle_stop_seconds(),
            startup_timeout_seconds: default_ssr_startup_timeout_seconds(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_backend")]
    pub backend: String,
    #[serde(default = "default_runtime_socket")]
    pub socket: String,
    #[serde(default = "default_build_image")]
    pub default_build_image: String,
    /// Image used to run SSR service containers.
    #[serde(default = "default_serve_image")]
    pub default_serve_image: String,
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
            default_serve_image: default_serve_image(),
            network: default_network(),
            resources: RuntimeResourcesConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrivateRepositoryTargetConfig {
    pub host: String,
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub private_repository_targets: Vec<PrivateRepositoryTargetConfig>,
}

impl SecurityConfig {
    pub fn repository_exceptions(&self) -> Vec<PrivateTargetException> {
        self.private_repository_targets
            .iter()
            .map(|target| PrivateTargetException {
                host: target.host.trim_end_matches('.').to_ascii_lowercase(),
                ip: target.ip,
                port: target.port,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevelopmentConfig {
    #[serde(default)]
    pub verbose_build_log: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NodeConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    #[serde(default)]
    pub config_revision: u64,
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
        let mut config = Self::load_persisted(path)?;
        apply_env(&mut config)?;
        Ok(config)
    }

    pub fn load_persisted(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        load_toml_or_default(path)
    }

    pub fn node_token_configured(&self) -> bool {
        let token = self.node.node_token.trim();
        !token.is_empty() && token != "change-me"
    }

    pub fn sync_configuration(&self) -> NodeConfiguration {
        NodeConfiguration {
            node: NodeIdentityConfiguration {
                id: self.node.id.clone(),
                control_api: self.node.control_api.clone(),
                work_root: self.node.work_root.clone(),
                capabilities: NodeCapabilities {
                    build: self.node.capabilities.build,
                    serve: self.node.capabilities.serve,
                },
            },
            build: NodeBuildConfiguration {
                concurrency: self.build.concurrency,
                command_timeout_seconds: self.build.command_timeout_seconds,
                retain_workspace_on_failure: self.build.retain_workspace_on_failure,
            },
            serve: NodeServeConfiguration {
                host: self.serve.host.to_string(),
                port: self.serve.port,
                public_base_url: self.serve.public_base_url.clone(),
                metadata_cache_ttl_seconds: self.serve.metadata_cache_ttl_seconds,
                artifact_cache_root: self.serve.artifact_cache_root.clone(),
                capacity: NodeServeCapacityConfiguration {
                    cpu_millicores: self.serve.capacity.cpu_millicores,
                    memory_mb: self.serve.capacity.memory_mb,
                    disk_mb: self.serve.capacity.disk_mb,
                    max_deployments: self.serve.capacity.max_deployments,
                },
                ssr: NodeSsrConfiguration {
                    idle_stop_seconds: self.serve.ssr.idle_stop_seconds,
                    startup_timeout_seconds: self.serve.ssr.startup_timeout_seconds,
                },
            },
            runtime: NodeRuntimeConfiguration {
                backend: self.runtime.backend.clone(),
                socket: self.runtime.socket.clone(),
                default_build_image: self.runtime.default_build_image.clone(),
                default_serve_image: self.runtime.default_serve_image.clone(),
                network: self.runtime.network.clone(),
                resources: NodeRuntimeResourcesConfiguration {
                    cpu_limit: self.runtime.resources.cpu_limit,
                    memory_mb: self.runtime.resources.memory_mb,
                },
            },
            security: NodeSecurityConfiguration {
                private_repository_targets: self
                    .security
                    .private_repository_targets
                    .iter()
                    .map(|target| NodePrivateRepositoryTargetConfiguration {
                        host: target.host.clone(),
                        ip: target.ip.to_string(),
                        port: target.port,
                    })
                    .collect(),
            },
            development: NodeDevelopmentConfiguration {
                verbose_build_log: self.development.verbose_build_log,
            },
            log: NodeLogConfiguration {
                level: self.log.level.clone(),
                format: match self.log.format {
                    LogFormat::Pretty => NodeLogFormat::Pretty,
                    LogFormat::Json => NodeLogFormat::Json,
                },
            },
        }
    }

    fn apply_sync_configuration(&mut self, desired: &NodeConfiguration) -> anyhow::Result<()> {
        self.node.id.clone_from(&desired.node.id);
        self.node.control_api.clone_from(&desired.node.control_api);
        self.node.work_root.clone_from(&desired.node.work_root);
        self.node.capabilities.build = desired.node.capabilities.build;
        self.node.capabilities.serve = desired.node.capabilities.serve;
        self.build.concurrency = desired.build.concurrency;
        self.build.command_timeout_seconds = desired.build.command_timeout_seconds;
        self.build.retain_workspace_on_failure = desired.build.retain_workspace_on_failure;
        self.serve.host = desired
            .serve
            .host
            .parse()
            .context("serve host must be an IPv4 or IPv6 address")?;
        self.serve.port = desired.serve.port;
        self.serve
            .public_base_url
            .clone_from(&desired.serve.public_base_url);
        self.serve.metadata_cache_ttl_seconds = desired.serve.metadata_cache_ttl_seconds;
        self.serve
            .artifact_cache_root
            .clone_from(&desired.serve.artifact_cache_root);
        self.serve.capacity.cpu_millicores = desired.serve.capacity.cpu_millicores;
        self.serve.capacity.memory_mb = desired.serve.capacity.memory_mb;
        self.serve.capacity.disk_mb = desired.serve.capacity.disk_mb;
        self.serve.capacity.max_deployments = desired.serve.capacity.max_deployments;
        self.serve.ssr.idle_stop_seconds = desired.serve.ssr.idle_stop_seconds;
        self.serve.ssr.startup_timeout_seconds = desired.serve.ssr.startup_timeout_seconds;
        self.runtime.backend.clone_from(&desired.runtime.backend);
        self.runtime.socket.clone_from(&desired.runtime.socket);
        self.runtime
            .default_build_image
            .clone_from(&desired.runtime.default_build_image);
        self.runtime
            .default_serve_image
            .clone_from(&desired.runtime.default_serve_image);
        self.runtime.network.clone_from(&desired.runtime.network);
        self.runtime.resources.cpu_limit = desired.runtime.resources.cpu_limit;
        self.runtime.resources.memory_mb = desired.runtime.resources.memory_mb;
        self.security.private_repository_targets = desired
            .security
            .private_repository_targets
            .iter()
            .map(|target| {
                Ok(PrivateRepositoryTargetConfig {
                    host: target.host.clone(),
                    ip: target
                        .ip
                        .parse()
                        .context("private repository target IP is invalid")?,
                    port: target.port,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        self.development.verbose_build_log = desired.development.verbose_build_log;
        self.log.level.clone_from(&desired.log.level);
        self.log.format = match desired.log.format {
            NodeLogFormat::Pretty => LogFormat::Pretty,
            NodeLogFormat::Json => LogFormat::Json,
        };
        self.validate()
    }

    pub fn persist_desired(
        path: impl AsRef<Path>,
        revision: u64,
        desired: &NodeConfiguration,
    ) -> anyhow::Result<()> {
        let path = path.as_ref();
        let mut config = Self::load_persisted(path)
            .with_context(|| format!("failed to load Node config from {}", path.display()))?;
        config.apply_sync_configuration(desired)?;
        config.config_revision = revision;
        save_toml(path, &config)
            .with_context(|| format!("failed to persist Node config to {}", path.display()))
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

    pub fn validate(&self) -> anyhow::Result<()> {
        let capabilities = &self.node.capabilities;
        if !capabilities.build && !capabilities.serve {
            anyhow::bail!("node must enable build or serve");
        }
        if capabilities.build && self.build.concurrency == 0 {
            anyhow::bail!("build concurrency must be positive when build capability is enabled");
        }
        if capabilities.serve {
            let base_url = url::Url::parse(&self.serve.public_base_url)
                .context("serve public_base_url must be an absolute HTTP(S) URL")?;
            if !matches!(base_url.scheme(), "http" | "https") || !base_url.has_host() {
                anyhow::bail!("serve public_base_url must be an absolute HTTP(S) URL");
            }
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
    // SSR service containers must share a network with the node process
    // (containerized nodes dial them by container IP); deployments override
    // this without editing the generated node config.
    overlay_string("GWNODE_RUNTIME_NETWORK", &mut config.runtime.network);
    overlay_string(
        "GWNODE_RUNTIME_SERVE_IMAGE",
        &mut config.runtime.default_serve_image,
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

const fn default_max_deployments() -> u32 {
    10
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

fn default_serve_image() -> String {
    "docker.io/library/node:22".to_owned()
}

const fn default_ssr_idle_stop_seconds() -> u64 {
    1800
}

const fn default_ssr_startup_timeout_seconds() -> u64 {
    90
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
    use std::{fs, time::SystemTime};

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

    #[test]
    fn private_repository_exceptions_require_exact_host_ip_and_port() {
        let config: NodeConfig = toml::from_str(
            r#"
            [security]
            [[security.private_repository_targets]]
            host = "git.internal"
            ip = "10.0.0.8"
            port = 2222
            "#,
        )
        .unwrap();

        let exceptions = config.security.repository_exceptions();
        assert_eq!(exceptions.len(), 1);
        assert_eq!(exceptions[0].host, "git.internal");
        assert_eq!(exceptions[0].ip.to_string(), "10.0.0.8");
        assert_eq!(exceptions[0].port, 2222);
    }

    #[test]
    fn desired_configuration_persistence_preserves_token_and_provenance() {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("grass-node-sync-{unique}.toml"));
        fs::write(
            &path,
            r#"
generated_by = "grass-control-api"

[node]
id = "node-a"
control_api = "https://control.example.test"
node_token = "existing-secret-token"
work_root = "/data/node"
"#,
        )
        .unwrap();
        let current = NodeConfig::load_persisted(&path).unwrap();
        let mut desired = current.sync_configuration();
        desired.build.concurrency = 4;
        desired.runtime.default_build_image = "registry.example/build:v2".to_owned();

        NodeConfig::persist_desired(&path, 7, &desired).unwrap();

        let persisted = NodeConfig::load_persisted(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(persisted.config_revision, 7);
        assert_eq!(persisted.node.node_token, "existing-secret-token");
        assert_eq!(persisted.generated_by.as_deref(), Some("grass-control-api"));
        assert_eq!(persisted.build.concurrency, 4);
        assert_eq!(
            persisted.runtime.default_build_image,
            "registry.example/build:v2"
        );
        let reported = serde_json::to_string(&persisted.sync_configuration()).unwrap();
        assert!(!reported.contains("existing-secret-token"));
        assert!(!reported.contains("node_token"));
    }
}
