use std::{collections::BTreeMap, fmt};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct GitCredentialEncryptionConfig {
    #[serde(default)]
    pub active_key_id: String,
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

impl fmt::Debug for GitCredentialEncryptionConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitCredentialEncryptionConfig")
            .field("active_key_id", &self.active_key_id)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl GitCredentialEncryptionConfig {
    pub fn active_key(&self) -> Result<(&str, Vec<u8>), &'static str> {
        if self.active_key_id.is_empty() {
            return Err("git credential master key is not configured");
        }
        self.key(&self.active_key_id)
            .map(|key| (self.active_key_id.as_str(), key))
    }

    pub fn key(&self, key_id: &str) -> Result<Vec<u8>, &'static str> {
        let encoded = self
            .keys
            .get(key_id)
            .ok_or("git credential encryption key is unavailable")?;
        let key = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| "git credential encryption key is invalid")?;
        if key.len() != 32 {
            return Err("git credential encryption key must contain 32 bytes");
        }
        Ok(key)
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecretsConfig {
    #[serde(default = "default_secret_key")]
    pub secret_key: String,
    #[serde(default)]
    pub git_credentials: GitCredentialEncryptionConfig,
}

impl fmt::Debug for SecretsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretsConfig")
            .field("secret_key", &"[REDACTED]")
            .field("git_credentials", &self.git_credentials)
            .finish()
    }
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            secret_key: default_secret_key(),
            git_credentials: GitCredentialEncryptionConfig::default(),
        }
    }
}

fn default_secret_key() -> String {
    "change-me".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_credential_keyring_requires_a_dedicated_256_bit_key() {
        let mut config = GitCredentialEncryptionConfig::default();
        assert!(config.active_key().is_err());

        config.active_key_id = "primary".to_owned();
        config
            .keys
            .insert("primary".to_owned(), URL_SAFE_NO_PAD.encode([7_u8; 32]));
        let (key_id, key) = config.active_key().unwrap();
        assert_eq!(key_id, "primary");
        assert_eq!(key, [7_u8; 32]);

        config
            .keys
            .insert("short".to_owned(), URL_SAFE_NO_PAD.encode([1_u8; 16]));
        assert!(config.key("short").is_err());

        let debug = format!("{config:?}");
        assert!(debug.contains("primary"));
        assert!(!debug.contains(&URL_SAFE_NO_PAD.encode([7_u8; 32])));
    }
}
