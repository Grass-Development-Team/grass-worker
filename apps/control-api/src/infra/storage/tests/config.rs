use super::super::validate_key;
use super::*;

#[test]
fn storage_config_defaults_to_local_and_rejects_unsafe_paths() {
    let config = StorageConfig::default();
    assert_eq!(config.backend, StorageBackendKind::Local);
    assert!(config.validate().is_ok());
    assert!(validate_key("../outside").is_err());
    assert!(validate_key("/absolute").is_err());
}

#[test]
fn minio_and_r2_require_explicit_endpoints() {
    for backend in [StorageBackendKind::Minio, StorageBackendKind::R2] {
        let config = StorageConfig {
            backend,
            bucket: "artifacts".to_owned(),
            region: "us-east-1".to_owned(),
            ..StorageConfig::default()
        };
        assert!(
            config.validate().is_err(),
            "{backend:?} accepted a missing endpoint"
        );
    }
}

#[test]
fn remote_storage_requires_an_absolute_local_node_root() {
    let config = StorageConfig {
        backend: StorageBackendKind::S3,
        local_root: "relative-node-root".to_owned(),
        endpoint: "https://s3.example.com".to_owned(),
        bucket: "artifacts".to_owned(),
        region: "us-east-1".to_owned(),
        ..StorageConfig::default()
    };

    assert!(config.validate().is_err());
}

#[test]
fn provider_default_regions_preserve_backend_semantics() {
    assert_eq!(StorageBackendKind::R2.default_region(), "auto");
    assert_eq!(StorageBackendKind::S3.default_region(), "us-east-1");
    assert_eq!(StorageBackendKind::Minio.default_region(), "us-east-1");
}
