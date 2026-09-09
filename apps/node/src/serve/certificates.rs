//! Certificate bundle synchronization for Serve Nodes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grass_node_protocol::CertificateBundle;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

use crate::{client::ControlApiClient, config::NodeConfig};

const CERTIFICATE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

pub fn spawn(client: ControlApiClient, config: NodeConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CERTIFICATE_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match client.certificate_bundles().await {
                Ok(response) => {
                    for bundle in response.bundles {
                        if let Err(error) =
                            install_bundle(Path::new(&config.serve.artifact_cache_root), &bundle)
                                .await
                        {
                            warn!(
                                operation = "node.serve.certificates.install_failed",
                                ingress_id = %bundle.ingress_id,
                                %error,
                                "failed to install certificate bundle"
                            );
                        }
                    }
                }
                Err(error) => warn!(
                    operation = "node.serve.certificates.refresh_failed",
                    %error,
                    "failed to refresh regional ingress certificates"
                ),
            }
        }
    })
}

pub async fn install_bundle(cache_root: &Path, bundle: &CertificateBundle) -> anyhow::Result<()> {
    let hostname = safe_hostname(&bundle.hostname)?;
    let directory = cache_root.join("certificates");
    tokio::fs::create_dir_all(&directory).await?;
    let certificate_path = directory.join(format!("{hostname}.fullchain.pem"));
    let private_key_path = directory.join(format!("{hostname}.key.pem"));
    atomic_write(&certificate_path, bundle.certificate_pem.as_bytes()).await?;
    if let Err(error) = atomic_write(&private_key_path, bundle.private_key_pem.as_bytes()).await {
        let _ = tokio::fs::remove_file(&certificate_path).await;
        return Err(error);
    }
    info!(
        operation = "node.serve.certificates.installed",
        ingress_id = %bundle.ingress_id,
        hostname = %bundle.hostname,
        "regional ingress certificate installed"
    );
    Ok(())
}

fn safe_hostname(hostname: &str) -> anyhow::Result<String> {
    let normalized = grass_validator::normalize_host(hostname)
        .map_err(|error| anyhow::anyhow!("invalid certificate hostname: {error}"))?;
    if normalized.len() > 253
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        anyhow::bail!("certificate hostname contains unsafe path characters");
    }
    Ok(normalized)
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temp_path = PathBuf::from(format!(
        "{}.tmp-{}",
        path.display(),
        uuid::Uuid::now_v7().simple()
    ));
    let result = async {
        let mut file = tokio::fs::File::create(&temp_path).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .await?;
        }
        drop(file);
        tokio::fs::rename(&temp_path, path).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn installs_bundles_atomically_with_private_permissions() {
        let root = tempfile::tempdir().unwrap();
        install_bundle(
            root.path(),
            &CertificateBundle {
                ingress_id: uuid::Uuid::now_v7(),
                hostname: "edge.example.com".to_owned(),
                certificate_pem: "CERT".to_owned(),
                private_key_pem: "KEY".to_owned(),
                issued_at_unix: None,
                expires_at_unix: None,
                revision: String::new(),
            },
        )
        .await
        .unwrap();
        let key = root.path().join("certificates/edge.example.com.key.pem");
        assert_eq!(tokio::fs::read_to_string(&key).await.unwrap(), "KEY");
        assert_eq!(
            tokio::fs::metadata(key).await.unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn rejects_unsafe_certificate_hostnames() {
        assert!(safe_hostname("../secret").is_err());
    }
}
