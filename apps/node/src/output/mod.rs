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

    // Server bundles produced by meta-frameworks.
    let next_server = project_root.join(".next/server").is_dir();
    let nuxt_server = project_root.join(".output/server").is_dir();
    let astro_server = project_root.join("dist/server/entry.mjs").is_file();
    let sveltekit_server =
        detection.framework == Framework::SvelteKit && detection.static_signal == Some(false);
    let remix_server = matches!(
        detection.framework,
        Framework::Remix | Framework::ReactRouter
    ) && (project_root.join("build/server/index.js").is_file()
        || project_root.join("build/index.js").is_file());

    let static_candidates: &[&str] = match detection.framework {
        Framework::Next => &["out"],
        Framework::Nuxt => &[".output/public", "dist"],
        Framework::SvelteKit => &["build"],
        Framework::Remix | Framework::ReactRouter => &["build"],
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
            if (next_server || nuxt_server || astro_server || sveltekit_server || remix_server)
                && !static_requested
            {
                InspectedRuntime::Ssr
            } else {
                InspectedRuntime::Static { directory }
            }
        }
        None if next_server || nuxt_server || astro_server || sveltekit_server || remix_server => {
            InspectedRuntime::Ssr
        }
        None => InspectedRuntime::Unknown,
    }
}

#[derive(Debug)]
pub struct GeneratedOutput {
    pub output_root: PathBuf,
    pub framework_name: String,
    pub framework_version: String,
    pub spa_fallback: bool,
    /// `static` or `ssr`; reported to the Control API on upload.
    pub runtime_kind: &'static str,
}

/// How a framework's SSR build maps into `.grass/output/server`.
struct SsrLayout {
    /// Directory copied verbatim to `.grass/output/server`.
    server_root: PathBuf,
    /// Extra `(source, destination-relative-to-output-root)` copies applied
    /// after the server tree (e.g. Next static assets into the standalone
    /// directory).
    overlays: Vec<(PathBuf, String)>,
    /// Entry file relative to the output root.
    entry: String,
    host_env: &'static str,
}

/// Resolves the SSR layout for the supported frameworks, or a clear error
/// describing what the build is missing.
fn ssr_layout(project_root: &Path, detection: &Detection) -> Result<SsrLayout, OutputError> {
    match detection.framework {
        Framework::Next => {
            let standalone = project_root.join(".next/standalone");
            if !standalone.is_dir() {
                return Err(OutputError::Unrecognized(
                    "Next.js produced a server build without .next/standalone; \
                     set output: 'standalone' in next.config.js (or remove a conflicting \
                     output setting) and redeploy"
                        .to_owned(),
                ));
            }
            // server.js sits at the standalone root for plain projects and
            // under the package path for monorepos.
            let server_js = find_file(&standalone, "server.js", 4).ok_or_else(|| {
                OutputError::Unrecognized(".next/standalone does not contain server.js".to_owned())
            })?;
            let relative = server_js
                .strip_prefix(&standalone)
                .map_err(|error| OutputError::Other(anyhow::anyhow!(error)))?
                .to_string_lossy()
                .replace('\\', "/");
            let app_prefix = match relative.rsplit_once('/') {
                Some((prefix, _)) => format!("server/{prefix}"),
                None => "server".to_owned(),
            };
            // The standalone tree expects .next/static and public next to
            // server.js; the build keeps them outside, so overlay them in.
            let overlays = vec![
                (
                    project_root.join(".next/static"),
                    format!("{app_prefix}/.next/static"),
                ),
                (project_root.join("public"), format!("{app_prefix}/public")),
            ];
            Ok(SsrLayout {
                server_root: standalone,
                overlays,
                entry: format!("server/{relative}"),
                host_env: "HOSTNAME",
            })
        }
        Framework::Astro => {
            let entry = project_root.join("dist/server/entry.mjs");
            if !entry.is_file() {
                return Err(OutputError::Unrecognized(
                    "Astro produced a server build without dist/server/entry.mjs; \
                     use the @astrojs/node adapter in standalone mode"
                        .to_owned(),
                ));
            }
            // Unlike Next standalone and Nuxt Nitro, the Astro node adapter
            // externalizes some runtime dependencies instead of bundling
            // them, so the project's node_modules ships next to the server
            // tree for module resolution to walk into.
            Ok(SsrLayout {
                server_root: project_root.join("dist"),
                overlays: vec![(project_root.join("node_modules"), "node_modules".to_owned())],
                entry: "server/server/entry.mjs".to_owned(),
                host_env: "HOST",
            })
        }
        Framework::Nuxt => {
            let entry = project_root.join(".output/server/index.mjs");
            if !entry.is_file() {
                return Err(OutputError::Unrecognized(
                    "Nuxt produced a server build without .output/server/index.mjs".to_owned(),
                ));
            }
            Ok(SsrLayout {
                server_root: project_root.join(".output"),
                overlays: Vec::new(),
                entry: "server/server/index.mjs".to_owned(),
                host_env: "HOST",
            })
        }
        Framework::SvelteKit => {
            let entry = project_root.join("build/index.js");
            if !entry.is_file() {
                return Err(OutputError::Unrecognized(
                    "SvelteKit adapter-node output is missing build/index.js".to_owned(),
                ));
            }
            Ok(SsrLayout {
                server_root: project_root.join("build"),
                overlays: Vec::new(),
                entry: "server/index.js".to_owned(),
                host_env: "HOST",
            })
        }
        Framework::Remix | Framework::ReactRouter => {
            let (server_root, entry) = if project_root.join("build/server/index.js").is_file() {
                (project_root.join("build"), "server/server/index.js")
            } else if project_root.join("build/index.js").is_file() {
                (project_root.join("build"), "server/index.js")
            } else {
                return Err(OutputError::Unrecognized(
                    "SSR framework output is missing build/server/index.js or build/index.js"
                        .to_owned(),
                ));
            };
            let overlays = ["public", "build/client"]
                .into_iter()
                .filter_map(|path| {
                    let source = project_root.join(path);
                    source.is_dir().then(|| (source, path.to_owned()))
                })
                .collect();
            Ok(SsrLayout {
                server_root,
                overlays,
                entry: entry.to_owned(),
                host_env: "HOST",
            })
        }
        Framework::Vite | Framework::Unknown => {
            Err(OutputError::RuntimeNotImplemented("This framework's SSR"))
        }
    }
}

/// Breadth-first search for a file name under `root`, bounded by depth.
fn find_file(root: &Path, name: &str, max_depth: usize) -> Option<PathBuf> {
    walkdir::WalkDir::new(root)
        .max_depth(max_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .find(|entry| entry.file_type().is_file() && entry.file_name().to_string_lossy() == name)
        .map(|entry| entry.into_path())
}

/// Generates `.grass/output` from a finished build.
///
/// Fails when the user project ships its own `.grass/output/output.toml`
/// (custom output is a later-stage capability) and when the build output is
/// neither a supported static site nor a supported SSR server bundle.
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
        InspectedRuntime::Ssr => {
            return generate_ssr_output(project_root, &detection, configured_output, build_command);
        }
        InspectedRuntime::Serverless => {
            return Err(OutputError::RuntimeNotImplemented("Serverless"));
        }
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

    // `200.html` is the explicit history-SPA fallback signal for ordinary
    // static outputs. Hash routers need no fallback because fragments never
    // reach the server. Nuxt SPA mode is explicit framework configuration.
    let spa_fallback = match detection.framework {
        Framework::Nuxt => detection.static_signal == Some(true),
        Framework::Vite
        | Framework::SvelteKit
        | Framework::Remix
        | Framework::ReactRouter
        | Framework::Astro
        | Framework::Next
        | Framework::Unknown => static_directory.join("200.html").is_file(),
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
        runtime_kind: "static",
    })
}

/// Assembles `.grass/output` for an SSR build: the framework's server tree
/// under `server/`, framework-specific asset overlays, and an `output.toml`
/// with the `[server]` section the serve path executes.
fn generate_ssr_output(
    project_root: &Path,
    detection: &Detection,
    configured_output: Option<&str>,
    build_command: Option<&str>,
) -> Result<GeneratedOutput, OutputError> {
    let layout = ssr_layout(project_root, detection)?;

    let output_root = project_root.join(".grass/output");
    if output_root.exists() {
        std::fs::remove_dir_all(&output_root)?;
    }
    let server_target = output_root.join("server");
    std::fs::create_dir_all(&server_target)?;
    copy_dir(&layout.server_root, &server_target)?;
    for (source, destination) in &layout.overlays {
        if source.is_dir() {
            let target = output_root.join(destination);
            std::fs::create_dir_all(&target)?;
            copy_dir(source, &target)?;
        }
    }

    let manifest = manifest::ssr_manifest(
        (detection.framework != Framework::Unknown).then(|| {
            (
                detection.framework.name(),
                detection.framework_version.as_str(),
            )
        }),
        manifest::ServerSection {
            entry: layout.entry.clone(),
            start_command: format!("node {}", layout.entry),
            port_env: "PORT".to_owned(),
            host_env: layout.host_env.to_owned(),
        },
        build_command,
        configured_output,
    );
    std::fs::write(
        output_root.join("output.toml"),
        manifest::to_toml(&manifest).map_err(OutputError::Other)?,
    )?;

    manifest::validate_manifest(&manifest, &output_root)
        .map_err(|error| OutputError::Other(anyhow::anyhow!(error)))?;

    Ok(GeneratedOutput {
        output_root,
        framework_name: detection.framework.name().to_owned(),
        framework_version: detection.framework_version.clone(),
        spa_fallback: false,
        runtime_kind: "ssr",
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
    fn vite_output_without_200_html_does_not_enable_spa_fallback() {
        let dir = project(&[
            ("package.json", r#"{"devDependencies":{"vite":"^6.0.0"}}"#),
            ("dist/index.html", "<html>app</html>"),
            ("dist/assets/app.js", "console.log(1)"),
        ]);

        let generated = generate_grass_output(&dir, None, Some("npm run build")).unwrap();
        assert_eq!(generated.framework_name, "vite");
        assert!(!generated.spa_fallback);
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
    fn vite_output_with_200_html_enables_spa_fallback() {
        let dir = project(&[
            ("package.json", r#"{"devDependencies":{"vite":"^6.0.0"}}"#),
            ("dist/index.html", "<html>app</html>"),
            ("dist/200.html", "<html>fallback signal</html>"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.framework_name, "vite");
        assert!(generated.spa_fallback);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn vanilla_output_requires_200_html_for_spa_fallback() {
        let without_signal = project(&[
            ("package.json", "{}"),
            ("dist/index.html", "<html>vanilla</html>"),
        ]);
        let generated = generate_grass_output(&without_signal, None, None).unwrap();
        assert_eq!(generated.framework_name, "unknown");
        assert!(!generated.spa_fallback);
        std::fs::remove_dir_all(without_signal).unwrap();

        let with_signal = project(&[
            ("package.json", "{}"),
            ("dist/index.html", "<html>vanilla</html>"),
            ("dist/200.html", "<html>fallback signal</html>"),
        ]);
        let generated = generate_grass_output(&with_signal, None, None).unwrap();
        assert!(generated.spa_fallback);
        std::fs::remove_dir_all(with_signal).unwrap();
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
    fn next_server_output_without_standalone_gets_a_clear_error() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"next":"15.0.0"}}"#),
            (".next/server/app.js", "server"),
        ]);

        let error = generate_grass_output(&dir, None, None).unwrap_err();
        assert!(error.to_string().contains("standalone"), "{error}");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn next_standalone_output_generates_ssr_grass_output() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"next":"15.0.0"}}"#),
            (".next/server/app.js", "server"),
            (".next/standalone/server.js", "require('http')"),
            (
                ".next/standalone/node_modules/next/package.json",
                "{\"name\":\"next\"}",
            ),
            (".next/static/chunks/main.js", "chunk"),
            ("public/favicon.ico", "icon"),
        ]);

        let generated = generate_grass_output(&dir, None, Some("npm run build")).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "next");
        assert!(generated.output_root.join("server/server.js").is_file());
        assert!(
            generated
                .output_root
                .join("server/.next/static/chunks/main.js")
                .is_file()
        );
        assert!(
            generated
                .output_root
                .join("server/public/favicon.ico")
                .is_file()
        );

        let manifest = manifest::parse_manifest(
            &std::fs::read_to_string(generated.output_root.join("output.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.runtime.kind, "ssr");
        let server = manifest.server.unwrap();
        assert_eq!(server.entry, "server/server.js");
        assert_eq!(server.start_command, "node server/server.js");
        assert_eq!(server.host_env, "HOSTNAME");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn astro_node_adapter_output_generates_ssr_grass_output() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"astro":"5.0.0"}}"#),
            ("astro.config.mjs", "export default { output: 'server' }"),
            ("dist/server/entry.mjs", "export {}"),
            ("dist/client/_astro/app.js", "client"),
            ("node_modules/piccolore/index.js", "module.exports = {}"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "astro");
        assert!(
            generated
                .output_root
                .join("server/server/entry.mjs")
                .is_file()
        );
        assert!(
            generated
                .output_root
                .join("server/client/_astro/app.js")
                .is_file()
        );
        // Externalized runtime deps resolve from node_modules at the root.
        assert!(
            generated
                .output_root
                .join("node_modules/piccolore/index.js")
                .is_file()
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nuxt_nitro_output_generates_ssr_grass_output() {
        let dir = project(&[
            ("package.json", r#"{"dependencies":{"nuxt":"3.15.0"}}"#),
            (".output/server/index.mjs", "export {}"),
            (".output/public/_nuxt/app.js", "client"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "nuxt");
        assert!(
            generated
                .output_root
                .join("server/server/index.mjs")
                .is_file()
        );
        assert!(
            generated
                .output_root
                .join("server/public/_nuxt/app.js")
                .is_file()
        );

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
    fn sveltekit_adapter_node_generates_ssr_grass_output() {
        let dir = project(&[
            (
                "package.json",
                r#"{"dependencies":{"@sveltejs/kit":"2.0.0","@sveltejs/adapter-node":"5.0.0"}}"#,
            ),
            ("build/index.js", "import adapter from 'adapter-node';"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "sveltekit");
        assert!(generated.output_root.join("server/index.js").is_file());
        let manifest = manifest::parse_manifest(
            &std::fs::read_to_string(generated.output_root.join("output.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.server.unwrap().entry, "server/index.js");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remix_ssr_output_uses_server_bundle_and_client_overlay() {
        let dir = project(&[
            (
                "package.json",
                r#"{"dependencies":{"@remix-run/node":"2.10.0"}}"#,
            ),
            ("build/server/index.js", "export default {}"),
            ("build/client/assets/app.js", "client"),
            ("public/favicon.ico", "icon"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "remix");
        assert!(
            generated
                .output_root
                .join("server/server/index.js")
                .is_file()
        );
        assert!(
            generated
                .output_root
                .join("build/client/assets/app.js")
                .is_file()
        );
        assert!(generated.output_root.join("public/favicon.ico").is_file());
        let manifest = manifest::parse_manifest(
            &std::fs::read_to_string(generated.output_root.join("output.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.server.unwrap().entry, "server/server/index.js");

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn react_router_ssr_output_is_detected_without_plain_react_router_dependency() {
        let dir = project(&[
            (
                "package.json",
                r#"{"dependencies":{"react-router":"7.0.0","@react-router/node":"7.0.0"}}"#,
            ),
            ("build/server/index.js", "export default {}"),
        ]);

        let generated = generate_grass_output(&dir, None, None).unwrap();
        assert_eq!(generated.runtime_kind, "ssr");
        assert_eq!(generated.framework_name, "react-router");
        assert!(
            generated
                .output_root
                .join("server/server/index.js")
                .is_file()
        );

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
