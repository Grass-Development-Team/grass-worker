//! Storage provider configuration and credential validation.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::StorageError;

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackendKind {
    #[default]
    Local,
    S3,
    Minio,
    R2,
}

impl StorageBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
            Self::Minio => "minio",
            Self::R2 => "r2",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "s3" => Some(Self::S3),
            "minio" => Some(Self::Minio),
            "r2" => Some(Self::R2),
            _ => None,
        }
    }

    pub fn default_region(self) -> &'static str {
        match self {
            Self::R2 => "auto",
            Self::Local | Self::S3 | Self::Minio => "us-east-1",
        }
    }
}

impl std::str::FromStr for StorageBackendKind {
    type Err = StorageError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| {
            StorageError::InvalidConfig(format!("unsupported storage backend: {value}"))
        })
    }
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageConfig {
    #[serde(default)]
    pub backend: StorageBackendKind,
    #[serde(default = "default_local_root")]
    pub local_root: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default)]
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub force_path_style: bool,
    #[serde(default)]
    pub allow_http: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackendKind::Local,
            local_root: default_local_root(),
            endpoint: String::new(),
            region: default_region(),
            bucket: String::new(),
            prefix: String::new(),
            force_path_style: false,
            allow_http: false,
        }
    }
}

impl StorageConfig {
    pub fn local(root: impl Into<String>) -> Self {
        Self {
            local_root: root.into(),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        let root = self.local_root.trim();
        if root.is_empty() || !Path::new(root).is_absolute() {
            return Err(StorageError::InvalidConfig(
                "local_root must be a non-empty absolute path".to_owned(),
            ));
        }
        match self.backend {
            StorageBackendKind::Local => {}
            StorageBackendKind::S3 | StorageBackendKind::Minio | StorageBackendKind::R2 => {
                if self.bucket.trim().is_empty() {
                    return Err(StorageError::InvalidConfig(
                        "bucket is required for an S3-compatible backend".to_owned(),
                    ));
                }
                if self.region.trim().is_empty() {
                    return Err(StorageError::InvalidConfig(
                        "region is required for an S3-compatible backend".to_owned(),
                    ));
                }
                if matches!(
                    self.backend,
                    StorageBackendKind::Minio | StorageBackendKind::R2
                ) && self.endpoint.trim().is_empty()
                {
                    return Err(StorageError::InvalidConfig(format!(
                        "endpoint is required for the {} backend",
                        self.backend.as_str()
                    )));
                }
                if let Some(endpoint) =
                    (!self.endpoint.trim().is_empty()).then_some(self.endpoint.trim())
                {
                    let parsed = url::Url::parse(endpoint).map_err(|error| {
                        StorageError::InvalidConfig(format!("endpoint is invalid: {error}"))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(StorageError::InvalidConfig(
                            "endpoint must use http or https".to_owned(),
                        ));
                    }
                    if parsed.scheme() == "http" && !self.allow_http {
                        return Err(StorageError::InvalidConfig(
                            "allow_http must be enabled for an http endpoint".to_owned(),
                        ));
                    }
                }
            }
        }

        if self.prefix.split('/').any(|part| part == "..") {
            return Err(StorageError::UnsafePath(self.prefix.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorageCredentials {
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub session_token: Option<String>,
}

impl StorageCredentials {
    pub fn is_configured(&self) -> bool {
        self.access_key_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .secret_access_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            || self
                .session_token
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

fn default_local_root() -> String {
    "/data".to_owned()
}

fn default_region() -> String {
    "us-east-1".to_owned()
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
