//! grass-assets — embedded Console build assets.
//!
//! Embeds `apps/console/dist/` at compile time via `rust-embed`.

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../apps/console/dist/"]
pub struct ConsoleAssets;

pub fn get(path: &str) -> Option<rust_embed::EmbeddedFile> {
    ConsoleAssets::get(path)
}
