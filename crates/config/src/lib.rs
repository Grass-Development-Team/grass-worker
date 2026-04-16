use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::io::Write;

const DEFAULT_CONFIG_FILE: &str = "config.toml";
const PLACEHOLDER_CONFIG: &str =
    "# Fill in the required settings in this file before starting grass-worker.\n";

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
    MissingDefaultConfig { path: PathBuf },
    PlaceholderConfig { path: PathBuf },
    MissingDefaultConfigAndWriteFailed {
        path: PathBuf,
        source: std::io::Error,
    },
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
            Self::MissingDefaultConfig { path } => write!(
                f,
                "missing required config at {}. A placeholder file was created; fill it in and retry.",
                path.display()
            ),
            Self::PlaceholderConfig { path } => write!(
                f,
                "config file at {} is still empty or contains only placeholder comments; fill it in and retry.",
                path.display()
            ),
            Self::MissingDefaultConfigAndWriteFailed { path, source } => write!(
                f,
                "missing required config at {} and failed to create the placeholder file: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::MissingDefaultConfigAndWriteFailed { source, .. } => Some(source),
            Self::InvalidArguments(_)
            | Self::MissingDefaultConfig { .. }
            | Self::PlaceholderConfig { .. } => None,
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
pub struct DevelopmentConfig {
    pub dev_server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub node: NodeConfig,
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
        Self::load_from_args_in_dir(args, &current_dir)
    }

    fn load_from_args_in_dir<I, S>(args: I, current_dir: &Path) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let default_path = current_dir.join(DEFAULT_CONFIG_FILE);
        let selection = resolve_config_path(args, current_dir)?;

        if let Some(explicit_path) = selection.explicit_path {
            match Self::load_from_path(&explicit_path) {
                Ok(config) => return Ok(config),
                Err(ConfigError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        match Self::load_from_path(&default_path) {
            Ok(config) => return Ok(config),
            Err(ConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        match write_placeholder_config(&default_path) {
            Ok(()) => Err(ConfigError::MissingDefaultConfig { path: default_path }),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                Self::load_from_path(&default_path)
            }
            Err(source) => Err(ConfigError::MissingDefaultConfigAndWriteFailed {
                path: default_path,
                source,
            }),
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        if is_effectively_empty_config(&contents) {
            return Err(ConfigError::PlaceholderConfig {
                path: path.to_path_buf(),
            });
        }

        toml::from_str(&contents).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
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

fn write_placeholder_config(path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(PLACEHOLDER_CONFIG.as_bytes())
}

fn is_effectively_empty_config(contents: &str) -> bool {
    contents
        .lines()
        .map(|line| line.trim_start_matches('\u{FEFF}').trim())
        .all(|line| line.is_empty() || line.starts_with('#'))
}

fn default_server_listen() -> SocketAddr {
    "127.0.0.1:3000".parse().unwrap()
}

fn default_node_listen() -> SocketAddr {
    "127.0.0.1:3001".parse().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::ffi::OsString;
    use tempfile::tempdir;

    #[test]
    fn defaults_use_expected_listen_addresses() {
        let config = AppConfig::defaults();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:3000");
        assert_eq!(config.node.listen.to_string(), "127.0.0.1:3001");
        assert!(config.development.is_none());
    }

    #[test]
    fn load_from_toml_supports_optional_development_section() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(
            &config_path,
            r#"
[server]
listen = "0.0.0.0:7000"

[node]
listen = "0.0.0.0:7001"

[development]
dev_server = "http://127.0.0.1:5173"
"#,
        )
        .unwrap();

        let config = AppConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.server.listen.to_string(), "0.0.0.0:7000");
        assert_eq!(config.node.listen.to_string(), "0.0.0.0:7001");
        assert_eq!(
            config.development.as_ref().unwrap().dev_server,
            "http://127.0.0.1:5173"
        );
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

[node]
listen = "0.0.0.0:7001"
"#,
        )
        .unwrap();

        let args = vec![
            OsString::from("grass-worker-api"),
            OsString::from("--config"),
            explicit_path.clone().into_os_string(),
        ];

        let config = AppConfig::load_from_args_in_dir(args, temp_dir.path()).unwrap();

        assert_eq!(config.server.listen.to_string(), "0.0.0.0:7000");
        assert_eq!(config.node.listen.to_string(), "0.0.0.0:7001");
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

[node]
listen = "127.0.0.1:4101"
"#,
        )
        .unwrap();

        let config = AppConfig::load_from_args_in_dir(
            ["grass-worker-node", "--config", "missing.toml"],
            temp_dir.path(),
        )
        .unwrap();

        assert_eq!(config.server.listen.to_string(), "127.0.0.1:4100");
        assert_eq!(config.node.listen.to_string(), "127.0.0.1:4101");
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
"#,
        )
        .unwrap();

        let args = vec![
            OsString::from("grass-worker-api"),
            OsString::from("--config"),
            explicit_dir.clone().into_os_string(),
        ];

        let error = AppConfig::load_from_args_in_dir(args, temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::Io { .. }));
        assert!(error.to_string().contains(&explicit_dir.display().to_string()));
    }

    #[test]
    fn load_from_args_creates_placeholder_default_and_errors_when_default_is_missing() {
        let temp_dir = tempdir().unwrap();

        let error = AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path())
            .unwrap_err();

        assert!(matches!(error, ConfigError::MissingDefaultConfig { .. }));
        assert_eq!(
            fs::read_to_string(temp_dir.path().join(DEFAULT_CONFIG_FILE)).unwrap(),
            PLACEHOLDER_CONFIG,
        );
    }

    #[test]
    fn load_from_args_rejects_unedited_placeholder_default_config() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::write(&default_path, PLACEHOLDER_CONFIG).unwrap();

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::PlaceholderConfig { .. }));
    }

    #[test]
    fn load_from_args_rejects_empty_default_config() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::write(&default_path, "").unwrap();

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::PlaceholderConfig { .. }));
    }

    #[test]
    fn load_from_args_rejects_comment_only_default_config() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::write(
            &default_path,
            "# generated placeholder\n\n# fill this in before starting\n",
        )
        .unwrap();

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::PlaceholderConfig { .. }));
    }

    #[test]
    fn load_from_args_rejects_bom_prefixed_comment_only_default_config() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);

        fs::write(
            &default_path,
            "\u{FEFF}# generated placeholder\n# fill this in before starting\n",
        )
        .unwrap();

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::PlaceholderConfig { .. }));
    }

    #[test]
    fn load_from_args_reports_missing_config_when_placeholder_write_fails() {
        let temp_dir = tempdir().unwrap();
        let missing_dir = temp_dir.path().join("missing");

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], &missing_dir).unwrap_err();

        assert!(matches!(
            error,
            ConfigError::MissingDefaultConfigAndWriteFailed { .. }
        ));
        let message = error.to_string();
        assert!(message.contains("missing required config"));
        assert!(message.contains("failed to create the placeholder file"));
    }

    #[test]
    fn write_placeholder_config_does_not_overwrite_existing_file() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        let real_config = r#"
[server]
listen = "127.0.0.1:4100"
"#;

        fs::write(&default_path, real_config).unwrap();

        let error = write_placeholder_config(&default_path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&default_path).unwrap(), real_config);
    }

    #[test]
    fn load_from_args_rejects_missing_config_value() {
        let temp_dir = tempdir().unwrap();
        let error = AppConfig::load_from_args_in_dir(["grass-worker-api", "--config"], temp_dir.path())
            .unwrap_err();

        assert!(matches!(error, ConfigError::InvalidArguments(_)));
    }

    #[test]
    fn load_from_args_rejects_unknown_flags() {
        let temp_dir = tempdir().unwrap();
        let error = AppConfig::load_from_args_in_dir(["grass-worker-api", "--verbose"], temp_dir.path())
            .unwrap_err();

        assert!(matches!(error, ConfigError::InvalidArguments(_)));
    }

    #[test]
    fn load_from_args_reports_invalid_toml_from_selected_file() {
        let temp_dir = tempdir().unwrap();
        let default_path = temp_dir.path().join(DEFAULT_CONFIG_FILE);
        fs::write(&default_path, "server = [").unwrap();

        let error =
            AppConfig::load_from_args_in_dir(["grass-worker-api"], temp_dir.path()).unwrap_err();

        assert!(matches!(error, ConfigError::Parse { .. }));
        assert!(error.to_string().contains(&default_path.display().to_string()));
    }
}
