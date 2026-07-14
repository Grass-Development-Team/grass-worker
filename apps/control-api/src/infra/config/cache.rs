use serde::{Deserialize, Serialize};

use grass_cache::CacheBackend;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CacheConfig {
    #[serde(default = "default_cache_backend")]
    pub backend: CacheBackend,
    #[serde(default = "default_redis_url", alias = "redis_url")]
    pub url: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            backend: default_cache_backend(),
            url: default_redis_url(),
        }
    }
}

const fn default_cache_backend() -> CacheBackend {
    CacheBackend::Redis
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379/0".to_owned()
}
