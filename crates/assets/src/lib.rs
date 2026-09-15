//! grass-assets — embedded Console build assets.
//!
//! Release builds embed `apps/console/dist/` via a profile-local `OUT_DIR` copy.
//! Development builds use a placeholder and a separate Vite development server.

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

pub fn get(path: &str) -> Option<rust_embed::EmbeddedFile> {
    ConsoleAssets::get(path)
}
