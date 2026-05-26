use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecretsConfig {
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            secret_key: default_secret_key(),
        }
    }
}

fn default_secret_key() -> String {
    "change-me".to_owned()
}
