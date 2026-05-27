use serde::{Deserialize, Serialize};

use grass_cache::CacheBackend;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub backend: CacheBackend,
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            backend: CacheBackend::default(),
            redis_url: default_redis_url(),
        }
    }
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/0".to_owned()
}
