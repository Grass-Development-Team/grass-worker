//! grass-assets — embedded Console build assets.
//!
//! Embeds `public/` at compile time via `rust-embed`.
//! The build pipeline copies `apps/console/dist/` here before compilation.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "public/"]
pub struct ConsoleAssets;

pub fn get(path: &str) -> Option<rust_embed::EmbeddedFile> {
    ConsoleAssets::get(path)
}
