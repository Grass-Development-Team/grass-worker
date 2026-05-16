use std::path::Path;

use config::{Config, File};
use serde::Deserialize;

use super::ConfigError;

pub fn load_toml_or_default<T>(path: impl AsRef<Path>) -> Result<T, ConfigError>
where
    T: for<'de> Deserialize<'de>,
{
    let cfg = Config::builder()
        .add_source(File::from(path.as_ref()).required(false))
        .build()?;

    Ok(cfg.try_deserialize()?)
}
