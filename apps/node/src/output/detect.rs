//! Framework detection from project sources.
//!
//! Detectors answer "what does this project look like" from package.json
//! dependencies and framework config files; the build output inspector in
//! [`super::inspect`] has the final word on what was actually produced.

use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    Vite,
    Next,
    Nuxt,
    SvelteKit,
    Astro,
    Unknown,
}

impl Framework {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Vite => "vite",
            Self::Next => "next",
            Self::Nuxt => "nuxt",
            Self::SvelteKit => "sveltekit",
            Self::Astro => "astro",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub framework: Framework,
    pub framework_version: String,
    /// Whether framework configuration indicates a static-capable output
    /// (`output: "export"`, `ssr: false`, adapter-static, `output:
    /// "static"`). `None` when no signal was found.
    pub static_signal: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: std::collections::HashMap<String, String>,
}

impl PackageJson {
    fn version_of(&self, name: &str) -> Option<String> {
        self.dependencies
            .get(name)
            .or_else(|| self.dev_dependencies.get(name))
            .cloned()
    }
}

fn read_config_containing(root: &Path, names: &[&str]) -> String {
    let mut merged = String::new();
    for name in names {
        if let Ok(content) = std::fs::read_to_string(root.join(name)) {
            merged.push_str(&content);
            merged.push('\n');
        }
    }
    merged
}

/// Detects the framework of the project at `root`.
pub fn detect(root: &Path) -> Detection {
    let package: PackageJson = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();

    // Priority: meta-frameworks over their underlying bundler.
    if let Some(version) = package.version_of("next") {
        let config = read_config_containing(
            root,
            &["next.config.js", "next.config.mjs", "next.config.ts"],
        );
        let static_signal = if config.contains("export") && config.contains("output") {
            Some(config.contains("\"export\"") || config.contains("'export'"))
        } else {
            None
        };
        return Detection {
            framework: Framework::Next,
            framework_version: version,
            static_signal,
        };
    }

    if let Some(version) = package.version_of("nuxt") {
        let config = read_config_containing(root, &["nuxt.config.js", "nuxt.config.ts"]);
        let static_signal = if config.contains("ssr") {
            Some(config.contains("ssr: false") || config.contains("ssr:false"))
        } else {
            None
        };
        return Detection {
            framework: Framework::Nuxt,
            framework_version: version,
            static_signal,
        };
    }

    if let Some(version) = package.version_of("@sveltejs/kit") {
        let uses_adapter_static = package.version_of("@sveltejs/adapter-static").is_some()
            || read_config_containing(root, &["svelte.config.js", "svelte.config.ts"])
                .contains("adapter-static");
        return Detection {
            framework: Framework::SvelteKit,
            framework_version: version,
            static_signal: Some(uses_adapter_static),
        };
    }

    if let Some(version) = package.version_of("astro") {
        let config = read_config_containing(root, &["astro.config.mjs", "astro.config.ts"]);
        // Astro defaults to static output when no server adapter is set.
        let static_signal = if config.contains("output") {
            Some(config.contains("\"static\"") || config.contains("'static'"))
        } else {
            Some(true)
        };
        return Detection {
            framework: Framework::Astro,
            framework_version: version,
            static_signal,
        };
    }

    if let Some(version) = package.version_of("vite") {
        return Detection {
            framework: Framework::Vite,
            framework_version: version,
            static_signal: Some(true),
        };
    }

    Detection {
        framework: Framework::Unknown,
        framework_version: String::new(),
        static_signal: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(package_json: &str, extra: &[(&str, &str)]) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grass-detect-{}", uuid::Uuid::now_v7().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), package_json).unwrap();
        for (name, content) in extra {
            std::fs::write(dir.join(name), content).unwrap();
        }
        dir
    }

    #[test]
    fn detects_vite_projects() {
        let dir = project(r#"{"devDependencies":{"vite":"^6.0.0"}}"#, &[]);
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::Vite);
        assert_eq!(detection.static_signal, Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn detects_next_static_export() {
        let dir = project(
            r#"{"dependencies":{"next":"15.0.0"}}"#,
            &[("next.config.js", "module.exports = { output: 'export' };\n")],
        );
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::Next);
        assert_eq!(detection.static_signal, Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn detects_nuxt_spa_mode() {
        let dir = project(
            r#"{"dependencies":{"nuxt":"3.15.0"}}"#,
            &[(
                "nuxt.config.ts",
                "export default defineNuxtConfig({ ssr: false })\n",
            )],
        );
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::Nuxt);
        assert_eq!(detection.static_signal, Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn detects_sveltekit_adapter_static() {
        let dir = project(
            r#"{"devDependencies":{"@sveltejs/kit":"2.0.0","@sveltejs/adapter-static":"3.0.0"}}"#,
            &[],
        );
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::SvelteKit);
        assert_eq!(detection.static_signal, Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn detects_astro_static_by_default() {
        let dir = project(r#"{"dependencies":{"astro":"5.0.0"}}"#, &[]);
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::Astro);
        assert_eq!(detection.static_signal, Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_projects_have_no_signal() {
        let dir = project(r#"{"dependencies":{"express":"4.0.0"}}"#, &[]);
        let detection = detect(&dir);
        assert_eq!(detection.framework, Framework::Unknown);
        assert_eq!(detection.static_signal, None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
