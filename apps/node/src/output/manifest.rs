//! Grass Output API v1 manifest (`.grass/output/output.toml`).

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const OUTPUT_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputManifest {
    pub version: u32,
    pub runtime: RuntimeSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<FrameworkSection>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "static")]
    pub static_site: Option<StaticSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MetadataSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSection {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkSection {
    pub name: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticSection {
    pub directory: String,
    #[serde(default)]
    pub spa_fallback: bool,
    #[serde(default)]
    pub not_found: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildSection {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub output_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSection {
    #[serde(default)]
    pub generated_by: String,
    #[serde(default)]
    pub generated_at: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("output.toml could not be parsed: {0}")]
    Parse(String),
    #[error("unsupported output api version {0}; this node supports version 1")]
    UnsupportedVersion(u32),
    #[error("runtime kind {0} is not implemented yet")]
    RuntimeNotImplemented(String),
    #[error("runtime kind {0} is not supported")]
    UnsupportedRuntime(String),
    #[error("static manifest is missing the [static] section")]
    MissingStaticSection,
    #[error("static directory {0} contains unsafe path segments")]
    UnsafeStaticDirectory(String),
    #[error("static directory {0} does not exist in the output")]
    MissingStaticDirectory(String),
    #[error("static directory {0} does not contain index.html")]
    MissingIndexHtml(String),
}

#[allow(dead_code)] // Consumed by the serve manifest reader in Milestone 10.
pub fn parse_manifest(content: &str) -> Result<OutputManifest, ManifestError> {
    toml::from_str(content).map_err(|error| ManifestError::Parse(error.to_string()))
}

/// Validates a manifest against the first-stage rules and, given the
/// unpacked output root, checks the static directory on disk.
pub fn validate_manifest(
    manifest: &OutputManifest,
    output_root: &Path,
) -> Result<(), ManifestError> {
    if manifest.version != OUTPUT_API_VERSION {
        return Err(ManifestError::UnsupportedVersion(manifest.version));
    }

    match manifest.runtime.kind.as_str() {
        "static" => {}
        kind @ ("ssr" | "hybrid" | "serverless" | "edge") => {
            return Err(ManifestError::RuntimeNotImplemented(kind.to_owned()));
        }
        other => return Err(ManifestError::UnsupportedRuntime(other.to_owned())),
    }

    let static_site = manifest
        .static_site
        .as_ref()
        .ok_or(ManifestError::MissingStaticSection)?;

    let directory = static_site.directory.trim();
    if directory.is_empty()
        || Path::new(directory).components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ManifestError::UnsafeStaticDirectory(directory.to_owned()));
    }

    let static_dir = output_root.join(directory);
    if !static_dir.is_dir() {
        return Err(ManifestError::MissingStaticDirectory(directory.to_owned()));
    }
    if !static_dir.join("index.html").is_file() {
        return Err(ManifestError::MissingIndexHtml(directory.to_owned()));
    }

    Ok(())
}

/// Builds the manifest written by the first-stage adapters.
pub fn static_manifest(
    framework: Option<(&str, &str)>,
    spa_fallback: bool,
    build_command: Option<&str>,
    output_directory: Option<&str>,
) -> OutputManifest {
    OutputManifest {
        version: OUTPUT_API_VERSION,
        runtime: RuntimeSection {
            kind: "static".to_owned(),
        },
        framework: framework.map(|(name, version)| FrameworkSection {
            name: name.to_owned(),
            version: version.to_owned(),
        }),
        static_site: Some(StaticSection {
            directory: "static".to_owned(),
            spa_fallback,
            not_found: String::new(),
        }),
        build: Some(BuildSection {
            command: build_command.unwrap_or_default().to_owned(),
            output_directory: output_directory.unwrap_or_default().to_owned(),
        }),
        metadata: Some(MetadataSection {
            generated_by: format!("grass-node/{}", env!("CARGO_PKG_VERSION")),
            generated_at: String::new(),
        }),
    }
}

pub fn to_toml(manifest: &OutputManifest) -> anyhow::Result<String> {
    toml::to_string_pretty(manifest).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output_root_with_index() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grass-manifest-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(dir.join("static")).unwrap();
        std::fs::write(dir.join("static/index.html"), "<html></html>").unwrap();
        dir
    }

    #[test]
    fn minimal_static_manifest_parses_and_validates() {
        let manifest = parse_manifest(
            r#"
version = 1

[runtime]
kind = "static"

[static]
directory = "static"
"#,
        )
        .unwrap();
        let root = output_root_with_index();
        assert!(validate_manifest(&manifest, &root).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_static_runtimes_fail_with_not_implemented() {
        for kind in ["ssr", "hybrid", "serverless", "edge"] {
            let manifest =
                parse_manifest(&format!("version = 1\n[runtime]\nkind = \"{kind}\"\n")).unwrap();
            let error = validate_manifest(&manifest, Path::new("/nonexistent")).unwrap_err();
            assert_eq!(error, ManifestError::RuntimeNotImplemented(kind.to_owned()));
            assert!(error.to_string().contains("not implemented yet"));
        }
    }

    #[test]
    fn unknown_versions_and_runtimes_are_rejected() {
        let manifest = parse_manifest("version = 2\n[runtime]\nkind = \"static\"\n").unwrap();
        assert_eq!(
            validate_manifest(&manifest, Path::new("/nonexistent")),
            Err(ManifestError::UnsupportedVersion(2))
        );

        let manifest = parse_manifest("version = 1\n[runtime]\nkind = \"wasm\"\n").unwrap();
        assert_eq!(
            validate_manifest(&manifest, Path::new("/nonexistent")),
            Err(ManifestError::UnsupportedRuntime("wasm".to_owned()))
        );
    }

    #[test]
    fn static_manifests_require_safe_existing_directories_with_index() {
        let root = output_root_with_index();

        let escape = parse_manifest(
            "version = 1\n[runtime]\nkind = \"static\"\n[static]\ndirectory = \"../outside\"\n",
        )
        .unwrap();
        assert!(matches!(
            validate_manifest(&escape, &root),
            Err(ManifestError::UnsafeStaticDirectory(_))
        ));

        let missing = parse_manifest(
            "version = 1\n[runtime]\nkind = \"static\"\n[static]\ndirectory = \"missing\"\n",
        )
        .unwrap();
        assert!(matches!(
            validate_manifest(&missing, &root),
            Err(ManifestError::MissingStaticDirectory(_))
        ));

        std::fs::remove_file(root.join("static/index.html")).unwrap();
        let manifest = parse_manifest(
            "version = 1\n[runtime]\nkind = \"static\"\n[static]\ndirectory = \"static\"\n",
        )
        .unwrap();
        assert!(matches!(
            validate_manifest(&manifest, &root),
            Err(ManifestError::MissingIndexHtml(_))
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_manifests_round_trip() {
        let manifest = static_manifest(
            Some(("vite", "6.0.0")),
            true,
            Some("npm run build"),
            Some("dist"),
        );
        let toml = to_toml(&manifest).unwrap();
        let parsed = parse_manifest(&toml).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.runtime.kind, "static");
        assert!(parsed.static_site.unwrap().spa_fallback);
        assert_eq!(parsed.framework.unwrap().name, "vite");
    }
}
