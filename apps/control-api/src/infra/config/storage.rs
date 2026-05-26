use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage_root")]
    pub root: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: default_storage_root(),
        }
    }
}

fn default_storage_root() -> String {
    "/data".to_owned()
}
