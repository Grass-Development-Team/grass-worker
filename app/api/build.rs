use std::path::Path;

fn main() {
    const SHARED_PUBLIC_DIR: &str = "../../crates/assets/assets/public";

    println!("cargo:rerun-if-changed={SHARED_PUBLIC_DIR}");

    if std::env::var("PROFILE").ok().as_deref() == Some("release") {
        let index_html = Path::new(SHARED_PUBLIC_DIR).join("index.html");

        if !index_html.is_file() {
            panic!(
                "missing embedded frontend assets at {}. Run `just frontend-build` before `cargo build --release`.",
                index_html.display()
            );
        }
    }
}
