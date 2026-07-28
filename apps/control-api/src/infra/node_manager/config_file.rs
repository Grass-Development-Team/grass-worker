//! Generation of the managed local `grass-node` configuration file.
//!
//! The file is written once at Node creation time because that is the only
//! moment the plaintext node token exists. It is marked with a
//! `generated_by` key so later automation (storage-root changes) only ever
//! rewrites files this module created, never a hand-written node.toml.

use std::{net::IpAddr, path::Path};

use anyhow::Context;
use serde::Serialize;

const GENERATED_BY: &str = "grass-control-api";

pub struct GenerateParams<'a> {
    pub node_name: &'a str,
    pub node_token: &'a str,
    pub control_api_url: String,
    pub storage_root: &'a str,
}

#[derive(Serialize)]
struct GeneratedConfig {
    generated_by: String,
    node: NodeSection,
    runtime: RuntimeSection,
    serve: ServeSection,
}

#[derive(Serialize)]
struct NodeSection {
    id: String,
    control_api: String,
    node_token: String,
    work_root: String,
    capabilities: CapabilitiesSection,
}

#[derive(Serialize)]
struct CapabilitiesSection {
    build: bool,
    serve: bool,
}

#[derive(Serialize)]
struct RuntimeSection {
    backend: String,
    socket: String,
    default_build_image: String,
}

#[derive(Serialize)]
struct ServeSection {
    host: String,
    port: u16,
    public_base_url: String,
    artifact_cache_root: String,
}

pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// The URL the local node should use to reach this Control API.
pub fn control_api_url(host: IpAddr, port: u16) -> String {
    let host = if host.is_unspecified() {
        "127.0.0.1".to_owned()
    } else {
        host.to_string()
    };
    format!("http://{host}:{port}")
}

fn node_work_root(storage_root: &str) -> String {
    format!("{}/node", storage_root.trim_end_matches('/'))
}

/// Writes the managed node config (mode 0600) and pre-creates its work
/// directories. Returns non-fatal warnings for the admin response.
pub fn generate(path: &str, params: &GenerateParams<'_>) -> anyhow::Result<Vec<String>> {
    let work_root = node_work_root(params.storage_root);
    let artifact_cache_root = format!("{work_root}/artifacts");

    let config = GeneratedConfig {
        generated_by: GENERATED_BY.to_owned(),
        node: NodeSection {
            id: params.node_name.to_owned(),
            control_api: params.control_api_url.clone(),
            node_token: params.node_token.to_owned(),
            work_root: work_root.clone(),
            capabilities: CapabilitiesSection {
                build: true,
                serve: true,
            },
        },
        runtime: detect_runtime(),
        serve: ServeSection {
            host: "0.0.0.0".to_owned(),
            port: 8080,
            public_base_url: "http://127.0.0.1:8080".to_owned(),
            artifact_cache_root: artifact_cache_root.clone(),
        },
    };

    grass_config::save_toml(path, &config)
        .with_context(|| format!("failed to write local node config {path}"))?;

    Ok(prepare_directories(&work_root, &artifact_cache_root))
}

/// Rewrites the work directories of a previously generated config after the
/// platform storage root changed. Hand-written configs are left untouched.
pub fn update_storage_root(path: &str, storage_root: &str) -> anyhow::Result<bool> {
    if !exists(path) {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read local node config {path}"))?;
    let mut value: toml::Value = raw
        .parse()
        .with_context(|| format!("failed to parse local node config {path}"))?;

    let generated = value
        .get("generated_by")
        .and_then(toml::Value::as_str)
        .is_some_and(|by| by == GENERATED_BY);
    if !generated {
        return Ok(false);
    }

    let work_root = node_work_root(storage_root);
    let artifact_cache_root = format!("{work_root}/artifacts");
    if let Some(node) = value.get_mut("node").and_then(toml::Value::as_table_mut) {
        node.insert(
            "work_root".to_owned(),
            toml::Value::String(work_root.clone()),
        );
    }
    if let Some(serve) = value.get_mut("serve").and_then(toml::Value::as_table_mut) {
        serve.insert(
            "artifact_cache_root".to_owned(),
            toml::Value::String(artifact_cache_root.clone()),
        );
    }

    grass_config::save_toml(path, &value)
        .with_context(|| format!("failed to rewrite local node config {path}"))?;
    prepare_directories(&work_root, &artifact_cache_root);
    Ok(true)
}

fn prepare_directories(work_root: &str, artifact_cache_root: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    for directory in [work_root, artifact_cache_root] {
        if let Err(error) = std::fs::create_dir_all(directory) {
            warnings.push(format!(
                "could not create {directory}: {error}; the local node cannot build or serve until this path is writable"
            ));
        }
    }
    warnings
}

/// Picks a container runtime socket that exists on this machine, preferring
/// an explicit DOCKER_HOST, then the Docker socket, then rootless Podman.
fn detect_runtime() -> RuntimeSection {
    let default_build_image = "docker.io/library/node:22".to_owned();

    if let Ok(host) = std::env::var("DOCKER_HOST") {
        if host.starts_with("unix://") {
            return RuntimeSection {
                backend: "docker-socket".to_owned(),
                socket: host,
                default_build_image,
            };
        }
    }
    if Path::new("/var/run/docker.sock").exists() {
        return RuntimeSection {
            backend: "docker-socket".to_owned(),
            socket: "unix:///var/run/docker.sock".to_owned(),
            default_build_image,
        };
    }
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let socket = format!("{}/podman/podman.sock", runtime_dir.trim_end_matches('/'));
        if Path::new(&socket).exists() {
            return RuntimeSection {
                backend: "podman-socket".to_owned(),
                socket: format!("unix://{socket}"),
                default_build_image,
            };
        }
    }
    RuntimeSection {
        backend: "podman-socket".to_owned(),
        socket: "unix:///run/user/1000/podman/podman.sock".to_owned(),
        default_build_image,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "grass-node-config-{label}-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn control_api_url_replaces_unspecified_host() {
        assert_eq!(
            control_api_url("0.0.0.0".parse().unwrap(), 7817),
            "http://127.0.0.1:7817"
        );
        assert_eq!(
            control_api_url("127.0.0.1".parse().unwrap(), 7817),
            "http://127.0.0.1:7817"
        );
    }

    #[test]
    fn generated_config_round_trips_and_marks_provenance() {
        let path = temp_path("generate");
        let storage = std::env::temp_dir().join("grass-node-config-storage-test");
        let params = GenerateParams {
            node_name: "local-node",
            node_token: "secret-token",
            control_api_url: "http://127.0.0.1:7817".to_owned(),
            storage_root: storage.to_str().unwrap(),
        };

        let warnings = generate(path.to_str().unwrap(), &params).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

        let raw = std::fs::read_to_string(&path).unwrap();
        let value: toml::Value = raw.parse().unwrap();
        assert_eq!(
            value.get("generated_by").and_then(toml::Value::as_str),
            Some("grass-control-api")
        );
        assert_eq!(
            value["node"]["node_token"].as_str().unwrap(),
            "secret-token"
        );
        assert_eq!(value["node"]["capabilities"]["build"].as_bool(), Some(true));
        assert_eq!(value["node"]["capabilities"]["serve"].as_bool(), Some(true));
        assert!(
            value["node"]["work_root"]
                .as_str()
                .unwrap()
                .ends_with("/node")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir_all(storage);
    }

    #[test]
    fn storage_root_update_only_touches_generated_configs() {
        let hand_written = temp_path("hand-written");
        std::fs::write(&hand_written, "[node]\nwork_root = \"/custom\"\n").unwrap();
        assert!(!update_storage_root(hand_written.to_str().unwrap(), "/tmp/grass-new").unwrap());
        std::fs::remove_file(&hand_written).unwrap();

        let generated = temp_path("generated");
        let storage_old = std::env::temp_dir().join("grass-node-config-old-root");
        let storage_new = std::env::temp_dir().join("grass-node-config-new-root");
        let params = GenerateParams {
            node_name: "local-node",
            node_token: "secret-token",
            control_api_url: "http://127.0.0.1:7817".to_owned(),
            storage_root: storage_old.to_str().unwrap(),
        };
        generate(generated.to_str().unwrap(), &params).unwrap();

        assert!(
            update_storage_root(generated.to_str().unwrap(), storage_new.to_str().unwrap())
                .unwrap()
        );
        let value: toml::Value = std::fs::read_to_string(&generated)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            value["node"]["work_root"].as_str().unwrap(),
            format!("{}/node", storage_new.to_str().unwrap())
        );
        assert_eq!(
            value["node"]["node_token"].as_str().unwrap(),
            "secret-token",
            "token must survive the rewrite"
        );

        std::fs::remove_file(&generated).unwrap();
        let _ = std::fs::remove_dir_all(storage_old);
        let _ = std::fs::remove_dir_all(storage_new);
    }
}
