use std::path::Path;

fn main() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let is_release = profile == "release";

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dist = workspace_root.join("../../apps/console/dist");
    let public = workspace_root.join("assets/public");

    let _ = std::fs::remove_dir_all(&public);
    std::fs::create_dir_all(&public).unwrap();

    if is_release && dist.is_dir() {
        copy_dir(&dist, &public).unwrap();
    } else {
        std::fs::write(
            public.join("index.html"),
            if is_release {
                "<!doctype html><html><body><p>Console not built. Run <code>just build console</code> first.</p></body></html>"
            } else {
                "<!doctype html><html><body><p>Frontend served by Vite dev server in debug mode.</p></body></html>"
            },
        )
        .unwrap();
    }
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
