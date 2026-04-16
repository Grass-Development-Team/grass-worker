use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const DEFAULT_CONFIG_FILE: &str = "config.toml";

#[derive(Debug)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    InvalidArguments(String),
    MissingSection { section: &'static str },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read config file {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse config file {}: {source}", path.display())
            }
            Self::InvalidArguments(message) => write!(f, "{message}"),
            Self::MissingSection { section } => {
                write!(f, "missing required [{section}] section in config file")
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::InvalidArguments(_) | Self::MissingSection { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_server_listen")]
    pub listen: SocketAddr,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: default_server_listen(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeConfig {
    #[serde(default = "default_node_listen")]
    pub listen: SocketAddr,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen: default_node_listen(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub db_name: String,
    pub user: String,
    pub password: String,
    #[serde(default = "default_database_schema")]
    pub schema: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: default_database_host(),
            port: default_database_port(),
            db_name: default_database_name(),
            user: default_database_user(),
            password: default_database_password(),
            schema: default_database_schema(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentConfig {
    pub dev_server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct ConfigFile {
    pub server: Option<ServerConfig>,
    pub node: Option<NodeConfig>,
    pub database: Option<DatabaseConfig>,
    pub development: Option<DevelopmentConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    pub database: Option<DatabaseConfig>,
    pub development: Option<DevelopmentConfig>,
}

impl AppConfig {
    pub fn defaults() -> Self {
        Self::default()
    }

    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_args(std::env::args_os())
    }

    pub fn load_from_args<I, S>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let current_dir = std::env::current_dir().map_err(|source| ConfigError::Io {
            path: PathBuf::from("."),
            source,
        })?;
        Self::load_for_api_in_dir(args, &current_dir)
    }

    pub fn load_for_api_in_dir<I, S>(args: I, current_dir: &Path) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let default_path = current_dir.join(DEFAULT_CONFIG_FILE);
        let selection = resolve_config_path(args, current_dir)?;

        if let Some(explicit_path) = selection.explicit_path {
            match Self::load_for_api_from_path(&explicit_path) {
                Ok(config) => return Ok(config),
                Err(ConfigError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        match Self::load_for_api_from_path(&default_path) {
            Ok(config) => return Ok(config),
            Err(ConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(Self::default())
            }
            Err(error) => return Err(error),
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Self::load_for_api_from_path(path)
    }

    fn load_for_api_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let config_file = load_config_file(path)?;
        Ok(Self {
            server: config_file.server.unwrap_or_default(),
            database: config_file.database,
            development: config_file.development,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeAppConfig {
    pub node: NodeConfig,
}

impl NodeAppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_args(std::env::args_os())
    }

    pub fn load_from_args<I, S>(args: I) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let current_dir = std::env::current_dir().map_err(|source| ConfigError::Io {
            path: PathBuf::from("."),
            source,
        })?;
        Self::load_in_dir(args, &current_dir)
    }

    pub fn load_in_dir<I, S>(args: I, current_dir: &Path) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let default_path = current_dir.join(DEFAULT_CONFIG_FILE);
        let selection = resolve_config_path(args, current_dir)?;

        if let Some(explicit_path) = selection.explicit_path {
            return Self::load_from_path(&explicit_path);
        }

        Self::load_from_path(default_path)
    }

    fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let config_file = load_config_file(path)?;
        let node = config_file
            .node
            .ok_or(ConfigError::MissingSection { section: "node" })?;
        Ok(Self { node })
    }
}

struct ConfigSelection {
    explicit_path: Option<PathBuf>,
}

fn resolve_config_path<I, S>(args: I, current_dir: &Path) -> Result<ConfigSelection, ConfigError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let mut explicit_path = None;

    while let Some(arg) = args.next() {
        if arg == "--config" {
            let value = args.next().ok_or_else(|| {
                ConfigError::InvalidArguments("missing value for --config".to_owned())
            })?;
            if explicit_path.is_some() {
                return Err(ConfigError::InvalidArguments(
                    "--config may only be provided once".to_owned(),
                ));
            }
            explicit_path = Some(path_from_arg(value, current_dir));
            continue;
        }

        return Err(ConfigError::InvalidArguments(format!(
            "unknown argument: {}",
            arg.to_string_lossy()
        )));
    }

    Ok(ConfigSelection { explicit_path })
}

fn path_from_arg(arg: OsString, current_dir: &Path) -> PathBuf {
    let path = PathBuf::from(arg);
    if path.is_absolute() {
        path
    } else {
        current_dir.join(path)
    }
}

fn load_config_file(path: impl AsRef<Path>) -> Result<ConfigFile, ConfigError> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn default_server_listen() -> SocketAddr {
    "127.0.0.1:3000".parse().unwrap()
}

fn default_node_listen() -> SocketAddr {
    "127.0.0.1:3001".parse().unwrap()
}

fn default_database_host() -> String {
    "127.0.0.1".to_owned()
}

fn default_database_port() -> u16 {
    5432
}

fn default_database_name() -> String {
    "grass_worker".to_owned()
}

fn default_database_user() -> String {
    "postgres".to_owned()
}

fn default_database_password() -> String {
    "postgres".to_owned()
}

fn default_database_schema() -> String {
    "public".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn defaults_use_expected_api_values() {
        let config = AppConfig::defaults();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:3000");
        assert!(config.database.is_none());
        assert!(config.development.is_none());
    }

    #[test]
    fn load_from_toml_supports_database_and_optional_development_section() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(
            &config_path,
            r#"
[server]
listen = "0.0.0.0:7000"

[database]
host = "db.internal"
port = 15432
db_name = "grass_worker"
user = "grass"
password = "secret"
schema = "control_plane"

[development]
dev_server = "http://127.0.0.1:5173"
"#,
        )
        .unwrap();

        let config = AppConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.server.listen.to_string(), "0.0.0.0:7000");
        assert_eq!(config.database.as_ref().unwrap().host, "db.internal");
        assert_eq!(config.database.as_ref().unwrap().port, 15432);
        assert_eq!(config.database.as_ref().unwrap().db_name, "grass_worker");
        assert_eq!(config.database.as_ref().unwrap().user, "grass");
        assert_eq!(config.database.as_ref().unwrap().password, "secret");
        assert_eq!(config.database.as_ref().unwrap().schema, "control_plane");
        assert_eq!(
            config.development.as_ref().unwrap().dev_server,
            "http://127.0.0.1:5173"
        );
    }

    #[test]
    fn load_from_toml_defaults_database_schema_to_public() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(
            &config_path,
            r#"
[server]
listen = "0.0.0.0:7000"

[database]
host = "db.internal"
port = 5432
db_name = "grass_worker"
user = "grass"
password = "secret"
"#,
        )
        .unwrap();

        let config = AppConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.database.as_ref().unwrap().schema, "public");
    }

    #[test]
    fn api_load_uses_default_server_when_config_file_is_missing() {
        let temp_dir = tempdir().unwrap();

        let config = AppConfig::load_for_api_in_dir(["grass-worker-api"], temp_dir.path()).unwrap();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:3000");
        assert!(config.database.is_none());
    }

    #[test]
    fn api_load_allows_config_without_database_section() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
[server]
listen = "0.0.0.0:7100"
"#,
        )
        .unwrap();

        let config = AppConfig::load_for_api_in_dir(["grass-worker-api"], temp_dir.path()).unwrap();

        assert_eq!(config.server.listen.to_string(), "0.0.0.0:7100");
        assert!(config.database.is_none());
    }

    #[test]
    fn node_load_requires_node_section() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        fs::write(
            &config_path,
            r#"
[server]
listen = "127.0.0.1:3000"
"#,
        )
        .unwrap();

        let error = NodeAppConfig::load_in_dir(["grass-worker-node"], temp_dir.path()).unwrap_err();

        assert!(matches!(
            error,
            ConfigError::MissingSection { section: "node" }
        ));
    }

    #[test]
    fn load_from_args_uses_explicit_config_when_it_exists() {
        let temp_dir = tempdir().unwrap();
        let explicit_path = temp_dir.path().join("custom.toml");
        fs::write(
            &explicit_path,
            r#"
[server]
listen = "0.0.0.0:7000"

[database]
host = "db.internal"
port = 5432
db_name = "grass_worker"
user = "grass"
password = "secret"
"#,
        )
        .unwrap();

        let args = vec![
            OsString::from("grass-worker-api"),
            OsString::from("--config"),
            explicit_path.clone().into_os_string(),
        ];

        let config = AppConfig::load_for_api_in_dir(args, temp_dir.path()).unwrap();

        assert_eq!(config.server.listen.to_string(), "0.0.0.0:7000");
        assert_eq!(config.database.as_ref().unwrap().host, "db.internal");
    }

    #[test]
    fn load_from_args_falls_back_to_default_when_explicit_path_is_missing() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(
            &default_path,
            r#"
[server]
listen = "127.0.0.1:4100"
"#,
        )
        .unwrap();

        let config =
            AppConfig::load_for_api_in_dir(["grass-worker-api", "--config", "missing.toml"], temp_dir.path())
                .unwrap();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:4100");
        assert!(config.database.is_none());
    }

    #[test]
    fn load_from_args_reports_io_error_for_selected_explicit_config() {
        let temp_dir = tempdir().unwrap();
        let explicit_dir = temp_dir.path().join("explicit-dir");
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::create_dir(&explicit_dir).unwrap();
        fs::write(
            &default_path,
            r#"
[server]
listen = "127.0.0.1:4100"

[node]
listen = "127.0.0.1:4101"

[database]
host = "db.internal"
port = 5432
db_name = "grass_worker"
user = "grass"
password = "secret"
"#,
        )
        .unwrap();

        let args = vec![
            OsString::from("grass-worker-api"),
            OsString::from("--config"),
            explicit_dir.clone().into_os_string(),
        ];

        let error = AppConfig::load_for_api_in_dir(args, temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::Io { .. }));
        assert!(error.to_string().contains(&explicit_dir.display().to_string()));
    }

    #[test]
    fn api_load_uses_defaults_when_server_section_is_missing() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::write(
            &default_path,
            r#"
[development]
dev_server = "http://127.0.0.1:5173"
"#,
        )
        .unwrap();

        let config = AppConfig::load_for_api_in_dir(["grass-worker-api"], temp_dir.path()).unwrap();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:3000");
        assert!(config.database.is_none());
        assert_eq!(
            config.development.as_ref().unwrap().dev_server,
            "http://127.0.0.1:5173"
        );
    }

    #[test]
    fn node_load_from_args_uses_explicit_config_when_present() {
        let temp_dir = tempdir().unwrap();
        let explicit_path = temp_dir.path().join("node.toml");
        fs::write(
            &explicit_path,
            r#"
[node]
listen = "0.0.0.0:7200"
"#,
        )
        .unwrap();

        let args = vec![
            OsString::from("grass-worker-node"),
            OsString::from("--config"),
            explicit_path.into_os_string(),
        ];

        let config = NodeAppConfig::load_in_dir(args, temp_dir.path()).unwrap();

        assert_eq!(config.node.listen.to_string(), "0.0.0.0:7200");
    }

    #[test]
    fn node_load_reports_missing_explicit_config_without_fallback() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(
            &default_path,
            r#"
[node]
listen = "127.0.0.1:4101"
"#,
        )
        .unwrap();

        let error = NodeAppConfig::load_in_dir(
            ["grass-worker-node", "--config", "missing.toml"],
            temp_dir.path(),
        )
        .unwrap_err();

        assert!(matches!(error, ConfigError::Io { .. }));
    }

    #[test]
    fn node_load_requires_config_file_to_exist() {
        let temp_dir = tempdir().unwrap();

        let error = NodeAppConfig::load_in_dir(["grass-worker-node"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::Io { .. }));
    }

    #[test]
    fn load_from_args_rejects_missing_config_value() {
        let temp_dir = tempdir().unwrap();
        let error = AppConfig::load_for_api_in_dir(["grass-worker-api", "--config"], temp_dir.path())
            .unwrap_err();

        assert!(matches!(error, ConfigError::InvalidArguments(_)));
    }

    #[test]
    fn load_from_args_rejects_unknown_flags() {
        let temp_dir = tempdir().unwrap();
        let error = AppConfig::load_for_api_in_dir(["grass-worker-api", "--verbose"], temp_dir.path())
            .unwrap_err();

        assert!(matches!(error, ConfigError::InvalidArguments(_)));
    }

    #[test]
    fn load_from_args_reports_invalid_toml_from_selected_file() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(&default_path, "server = [").unwrap();

        let error = AppConfig::load_for_api_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::Parse { .. }));
        assert!(error.to_string().contains(&default_path.display().to_string()));
    }
}
