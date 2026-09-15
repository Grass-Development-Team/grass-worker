use std::path::{Path, PathBuf};

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dist = manifest.join("../../apps/console/dist");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo provides OUT_DIR"));
    let public = output.join("public");

    println!("cargo:rerun-if-changed=build.rs");
    // Watching the directory includes additions, removals and nested assets.
    println!("cargo:rerun-if-changed={}", dist.display());

    let is_release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if is_release {
        let index = dist.join("index.html");
        assert!(
            index
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.len() > 0),
            "Console release assets are missing: run `just build console` or `just release` first"
        );
    }

    if public.exists() {
        std::fs::remove_dir_all(&public).expect("old generated assets can be removed");
    }
    std::fs::create_dir_all(&public).expect("generated asset directory can be created");

    if is_release {
        copy_dir(&dist, &public).expect("Console release assets can be copied");
    } else {
        std::fs::write(
            public.join("index.html"),
            "<!doctype html><html><body><p>Frontend served by Vite dev server in debug mode.</p></body></html>",
        )
        .expect("development placeholder can be written");
    }

    // Generate the literal path without adding path-interpolation dependencies.
    // OUT_DIR belongs to this Cargo target/profile and never modifies crate sources.
    std::fs::write(
        output.join("embedded.rs"),
        format!(
            "#[derive(rust_embed::RustEmbed)]\n#[folder = {:?}]\npub struct ConsoleAssets;\n",
            public
        ),
    )
    .expect("embedded asset declaration can be written");
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&dest)?;
            copy_dir(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    Ok(())
}
