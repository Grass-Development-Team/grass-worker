//! Grass Output generation: manifest, framework detection, build output
//! inspection, and adapters that normalize framework builds into
//! `.grass/output`.

pub mod detect;
pub mod manifest;

use std::path::{Path, PathBuf};

use detect::{Detection, Framework};

#[derive(Debug, thiserror::Error)]
pub enum OutputError {
    #[error("Custom Grass Output is not supported in the first stage")]
    CustomOutputUnsupported,
    #[error("{0} runtime is not implemented yet")]
    RuntimeNotImplemented(&'static str),
    #[error("build output was not recognized: {0}")]
    Unrecognized(String),
    #[error("static output directory {0} does not contain index.html")]
    MissingIndexHtml(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result of inspecting a finished build before adaptation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectedRuntime {
    Static { directory: PathBuf },
    Ssr,
    Serverless,
    Edge,
    Unknown,
}

/// Inspects the build output under `project_root` and decides the runtime
/// kind that was actually produced. Server outputs win over static
/// directories so an SSR build with a stray `index.html` still fails.
pub fn inspect_build_output(
    project_root: &Path,
    configured_output: Option<&str>,
    detection: &Detection,
) -> InspectedRuntime {
    // Serverless / edge outputs (Vercel build output layout).
    if project_root.join(".vercel/output/functions").is_dir() {
        return InspectedRuntime::Serverless;
    }
    if project_root.join("middleware.js").is_file() || project_root.join("middleware.ts").is_file()
    {
        return InspectedRuntime::Edge;
    }

    // Server bundles produced by meta-frameworks.
    let next_server = project_root.join(".next/server").is_dir();
    let nuxt_server = project_root.join(".output/server").is_dir();
    let sveltekit_server =
        detection.framework == Framework::SvelteKit && detection.static_signal == Some(false);

    let static_candidates: &[&str] = match detection.framework {
        Framework::Next => &["out"],
        Framework::Nuxt => &[".output/public", "dist"],
        Framework::SvelteKit => &["build"],
        Framework::Astro => &["dist"],
        Framework::Vite => &["dist"],
        Framework::Unknown => &["dist", "build", "out", "public", "_site"],
    };

    let mut candidates: Vec<String> = Vec::new();
    if let Some(configured) = configured_output.filter(|value| !value.trim().is_empty()) {
        candidates.push(configured.trim().to_owned());
    }
    candidates.extend(static_candidates.iter().map(|value| (*value).to_owned()));

    let static_directory = candidates.into_iter().find_map(|candidate| {
        let path = project_root.join(&candidate);
        (path.is_dir() && path.join("index.html").is_file()).then_some(path)
    });

    match static_directory {
        Some(directory) => {
            // A static export directory takes priority for frameworks whose
            // static mode was requested; otherwise a server bundle means SSR.
            let static_requested = detection.static_signal == Some(true);
            if (next_server || nuxt_server || sveltekit_server) && !static_requested {
                InspectedRuntime::Ssr
            } else {
                InspectedRuntime::Static { directory }
            }
        }
        None if next_server || nuxt_server || sveltekit_server => InspectedRuntime::Ssr,
        None => InspectedRuntime::Unknown,
    }
}

#[derive(Debug)]
pub struct GeneratedOutput {
    pub output_root: PathBuf,
    pub framework_name: String,
    pub framework_version: String,
    pub spa_fallback: bool,
}

/// Generates `.grass/output` from a finished build.
///
/// Fails when the user project ships its own `.grass/output/output.toml`
/// (custom output is a later-stage capability) and when the build output is
/// not a supported static site.
pub fn generate_grass_output(
    project_root: &Path,
    configured_output: Option<&str>,
    build_command: Option<&str>,
) -> Result<GeneratedOutput, OutputError> {
    if project_root.join(".grass/output/output.toml").is_file() {
        return Err(OutputError::CustomOutputUnsupported);
    }

    let detection = detect::detect(project_root);
    let inspected = inspect_build_output(project_root, configured_output, &detection);

    let static_directory = match inspected {
        InspectedRuntime::Static { directory } => directory,
        InspectedRuntime::Ssr => return Err(OutputError::RuntimeNotImplemented("SSR")),
        InspectedRuntime::Serverless => {
            return Err(OutputError::RuntimeNotImplemented("Serverless"));
        }
        InspectedRuntime::Edge => return Err(OutputError::RuntimeNotImplemented("Edge")),
        InspectedRuntime::Unknown => {
            return Err(OutputError::Unrecognized(
                "no static output directory with index.html was found".to_owned(),
            ));
        }
    };

    if !static_directory.join("index.html").is_file() {
        return Err(OutputError::MissingIndexHtml(
            static_directory.display().to_string(),
        ));
    }

    // SPA fallback: single-page frameworks route on the client, so unknown
    // paths return index.html. Prerendered multi-page outputs (Next export,
    // Astro, SvelteKit prerender) should 404 unless a 200.html exists.
    let spa_fallback = match detection.framework {
        Framework::Vite => true,
        Framework::Nuxt => detection.static_signal == Some(true),
        Framework::SvelteKit | Framework::Astro | Framework::Next => {
            static_directory.join("200.html").is_file()
        }
        Framework::Unknown => {
            static_directory.join("200.html").is_file()
                || !static_directory.join("404.html").is_file()
        }
    };

    let output_root = project_root.join(".grass/output");
    let static_target = output_root.join("static");
    if output_root.exists() {
        std::fs::remove_dir_all(&output_root)?;
    }
    std::fs::create_dir_all(&static_target)?;
    copy_dir(&static_directory, &static_target)?;

    let manifest = manifest::static_manifest(
        (detection.framework != Framework::Unknown).then(|| {
            (
                detection.framework.name(),
                detection.framework_version.as_str(),
            )
        }),
        spa_fallback,
        build_command,
        configured_output,
    );
    std::fs::write(
        output_root.join("output.toml"),
        manifest::to_toml(&manifest).map_err(OutputError::Other)?,
    )?;

    std::fs::create_dir_all(output_root.join("metadata"))?;
    write_checksums(&static_target, &output_root.join("metadata/checksums.toml"))?;

    manifest::validate_manifest(&manifest, &output_root)
        .map_err(|error| OutputError::Other(anyhow::anyhow!(error)))?;

    Ok(GeneratedOutput {
        output_root,
        framework_name: detection.framework.name().to_owned(),
        framework_version: detection.framework_version,
        spa_fallback,
    })
}

fn copy_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        let entry = entry.map_err(std::io::Error::other)?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(std::io::Error::other)?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &destination)?;
        }
        // Symlinks are skipped: artifacts must not capture host files.
    }
    Ok(())
}

fn write_checksums(static_dir: &Path, destination: &Path) -> std::io::Result<()> {
    use sha2::{Digest, Sha256};

    let mut lines = vec!["[files]".to_owned()];
    let mut entries: Vec<_> = walkdir::WalkDir::new(static_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    entries.sort();

    for path in entries {
        let relative = path
            .strip_prefix(static_dir)
            .map_err(std::io::Error::other)?;
        let name = relative.to_string_lossy().replace('\\', "/");
        let digest = hex::encode(Sha256::digest(std::fs::read(&path)?));
        lines.push(format!("\"{name}\" = \"{digest}\""));
    }

    std::fs::write(destination, lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grass-output-{}", uuid::Uuid::now_v7().simple()));
        for (name, content) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    #[test]
    fn vite_spa_output_generates_static_grass_output() {
        let dir = project(&[
            ("package.json", r#"{"devDependencies":{"vite":"^6.0.0"}}"#),
            ("dist/index.html", "<html>app</html>"),
            ("dist/assets/app.js", "console.log(1)"),
        ]);

        let generated = generate_grass_output(&dir, None, Some("npm run build")).unwrap();
        assert_eq!(generated.framework_name, "vite");
        assert!(generated.spa_fallback);
        assert!(generated.output_root.join("output.toml").is_file());
        assert!(generated.output_root.join("static/index.html").is_file());
        assert!(generated.output_root.join("static/assets/app.js").is_file());
        assert!(
            generated
                .output_root
                .join("metadata/checksums.toml")
                .is_file()
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn next_static_export_uses_out_directory() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"next":"15.0.0"}}"#),
            ("next.config.js", "module.exports = { output: 'export' };"),
            ("out/index.html", "<html>next</html>"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.framework_name, "next");
        assert!(!generated.spa_fallback);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn next_server_output_fails_as_ssr() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"next":"15.0.0"}}"#),
            (".next/server/app.js", "server"),
        ]);

        let error = generate_grass_output(&dir, None, None).unwrap_err();
        assert!(matches!(error, OutputError::RuntimeNotImplemented("SSR")));
        assert!(error.to_string().contains("not implemented yet"));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nuxt_static_output_uses_output_public() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"nuxt":"3.15.0"}}"#),
            ("nuxt.config.ts", "export default { ssr: false }"),
            (".output/public/index.html", "<html>nuxt</html>"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.framework_name, "nuxt");
        assert!(generated.spa_fallback);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sveltekit_adapter_static_uses_build_directory() {
        let dir = project(&[
            (
                "package.json",
                r#"{"devDependencies":{"@sveltejs/kit":"2.0.0","@sveltejs/adapter-static":"3.0.0"}}"#,
            ),
            ("build/index.html", "<html>kit</html>"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.framework_name, "sveltekit");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn astro_static_uses_dist() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"astro":"5.0.0"}}"#),
            ("dist/index.html", "<html>astro</html>"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.framework_name, "astro");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn serverless_output_fails_with_stable_message() {
        let dir = project(&[
            ("package.json", "{}"),
            (".vercel/output/functions/api.func/index.js", "handler"),
            ("dist/index.html", "<html></html>"),
        ]);

        let error = generate_grass_output(&dir, None, None).unwrap_err();
        assert!(matches!(
            error,
            OutputError::RuntimeNotImplemented("Serverless")
        ));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn custom_grass_output_is_rejected_in_the_first_stage() {
        let dir = project(&[
            (".grass/output/output.toml", "version = 1"),
            ("dist/index.html", "<html></html>"),
        ]);

        let error = generate_grass_output(&dir, None, None).unwrap_err();
        assert!(matches!(error, OutputError::CustomOutputUnsupported));
        assert_eq!(
            error.to_string(),
            "Custom Grass Output is not supported in the first stage"
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_output_without_index_fails() {
        let dir = project(&[("package.json", "{}"), ("dist/readme.md", "no html here")]);
        let error = generate_grass_output(&dir, None, None).unwrap_err();
        assert!(matches!(error, OutputError::Unrecognized(_)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn configured_output_directory_wins() {
        let dir = project(&[
            ("package.json", r#"{"devDependencies":{"vite":"^6.0.0"}}"#),
            ("custom-dist/index.html", "<html>custom</html>"),
        ]);

        let generated = generate_grass_output(&dir, Some("custom-dist"), None).unwrap();
        assert!(generated.output_root.join("static/index.html").is_file());

        std::fs::remove_dir_all(dir).unwrap();
    }
}
