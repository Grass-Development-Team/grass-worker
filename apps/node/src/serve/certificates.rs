//! Authoritative ingress snapshots, validated certificate cache and live SNI resolution.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use grass_node_protocol::{
    CertificateBundle, CertificateBundlesResponse, HttpChallenge, InstalledCertificate,
    ReportIngressStatusRequest,
};
use rustls::{
    pki_types::ServerName,
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tracing::warn;
use uuid::Uuid;

use crate::{
    client::{ControlApiClient, RouteSnapshotError},
    config::NodeConfig,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_ENTRIES: usize = 4096;
const MAX_CERTIFICATE_BYTES: usize = 1024 * 1024;
const MAX_PRIVATE_KEY_BYTES: usize = 64 * 1024;
pub const CHALLENGE_PREFIX: &str = "/.well-known/acme-challenge/";

#[derive(Clone)]
struct LoadedCertificate {
    bundle: CertificateBundle,
    key: Arc<CertifiedKey>,
    not_before: i64,
    not_after: i64,
}

impl LoadedCertificate {
    fn valid_at(&self, now: i64) -> bool {
        self.not_before <= now && now < self.not_after
    }
}

#[derive(Default)]
struct Snapshot {
    certificates: HashMap<String, LoadedCertificate>,
    challenges: HashMap<(String, String), HttpChallenge>,
    challenge_revision: String,
}

/// Debug intentionally excludes keys, certificates and HTTP challenge values.
pub struct IngressState {
    snapshot: RwLock<Snapshot>,
    update: tokio::sync::Mutex<()>,
    cache_path: PathBuf,
    node_id: Uuid,
    region: String,
    tls_enabled: bool,
    cache_dirty: AtomicBool,
    pub http_ready: AtomicBool,
    pub tls_ready: AtomicBool,
}

impl std::fmt::Debug for IngressState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IngressState")
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
struct CachedCertificates {
    node_id: Uuid,
    region: String,
    bundles: Vec<CertificateBundle>,
}

impl IngressState {
    pub fn new(config: &NodeConfig, node_id: Uuid) -> Self {
        Self {
            snapshot: RwLock::new(Snapshot::default()),
            update: tokio::sync::Mutex::new(()),
            cache_path: Path::new(&config.serve.artifact_cache_root)
                .join("certificates/active.json"),
            node_id,
            region: config.node.region.clone(),
            tls_enabled: config.serve.tls.enabled,
            cache_dirty: AtomicBool::new(true),
            http_ready: AtomicBool::new(false),
            tls_ready: AtomicBool::new(false),
        }
    }

    /// Only a previously committed snapshot for this registered node and region
    /// may be restored. Loose PEM files and stale temporary files are never used.
    pub async fn restore(&self) -> anyhow::Result<()> {
        let _update = self.update.lock().await;
        let bytes = match tokio::fs::read(&self.cache_path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let cached: CachedCertificates =
            serde_json::from_slice(&bytes).context("invalid certificate cache")?;
        if cached.node_id != self.node_id || cached.region != self.region {
            return Ok(());
        }
        if cached.bundles.len() > MAX_ENTRIES {
            anyhow::bail!("certificate cache exceeds the entry limit");
        }
        let now = unix_now();
        let mut certificates = HashMap::new();
        for bundle in cached.bundles {
            if let Ok(certificate) = validate_bundle(bundle, now) {
                certificates.insert(certificate.bundle.hostname.clone(), certificate);
            }
        }
        self.snapshot
            .write()
            .expect("ingress state poisoned")
            .certificates = certificates;
        Ok(())
    }

    /// Apply a complete snapshot. Invalid replacements retain the same host's
    /// previous valid certificate only when its opaque certificate ID matches.
    /// Omissions revoke both live certificates and their persisted cache entry.
    pub async fn apply(&self, response: CertificateBundlesResponse) -> anyhow::Result<()> {
        self.apply_at(response, unix_now()).await
    }

    async fn apply_at(&self, response: CertificateBundlesResponse, now: i64) -> anyhow::Result<()> {
        let _update = self.update.lock().await;
        if response.bundles.len() > MAX_ENTRIES || response.challenges.len() > MAX_ENTRIES {
            anyhow::bail!("ingress snapshot exceeds the entry limit");
        }
        let mut hosts = HashSet::new();
        let mut identifiers = HashSet::new();
        for bundle in &response.bundles {
            let host = safe_hostname(&bundle.hostname)?;
            if !hosts.insert(host) || !identifiers.insert(bundle.ingress_id) {
                anyhow::bail!("ingress snapshot contains duplicate certificates");
            }
        }
        let previous = self
            .snapshot
            .read()
            .expect("ingress state poisoned")
            .certificates
            .clone();
        let mut certificates = HashMap::new();
        for bundle in response.bundles {
            let host = safe_hostname(&bundle.hostname)?;
            let ingress_id = bundle.ingress_id;
            if let Some(certificate) = previous.get(&host).filter(|certificate| {
                certificate.valid_at(now)
                    && certificate.bundle.ingress_id == ingress_id
                    && certificate.bundle.certificate_pem == bundle.certificate_pem
                    && certificate.bundle.private_key_pem == bundle.private_key_pem
                    && (bundle.revision.is_empty()
                        || certificate.bundle.revision == bundle.revision)
            }) {
                certificates.insert(host, certificate.clone());
                continue;
            }
            match validate_bundle(bundle, now) {
                Ok(certificate) => {
                    certificates.insert(host, certificate);
                }
                Err(error) => {
                    warn!(operation = "node.serve.certificates.rejected", %ingress_id, %error, "certificate update rejected");
                    if let Some(certificate) = previous.get(&host).filter(|certificate| {
                        certificate.bundle.ingress_id == ingress_id && certificate.valid_at(now)
                    }) {
                        certificates.insert(host, certificate.clone());
                    }
                }
            }
        }
        let (challenges, challenge_revision) = match validate_challenges(
            response.challenges,
            response.challenge_revision,
            now,
        ) {
            Ok(challenges) => challenges,
            Err(error) => {
                warn!(operation = "node.serve.challenges.rejected", %error, "HTTP challenge snapshot rejected");
                (HashMap::new(), String::new())
            }
        };
        let cached = CachedCertificates {
            node_id: self.node_id,
            region: self.region.clone(),
            bundles: certificates
                .values()
                .map(|certificate| certificate.bundle.clone())
                .collect(),
        };
        let changed = certificates.len() != previous.len()
            || certificates.iter().any(|(host, certificate)| {
                previous.get(host).is_none_or(|old| {
                    old.bundle.ingress_id != certificate.bundle.ingress_id
                        || old.bundle.revision != certificate.bundle.revision
                })
            });
        let persisted = if changed || self.cache_dirty.load(Ordering::Acquire) {
            atomic_write(&self.cache_path, &serde_json::to_vec(&cached)?).await
        } else {
            Ok(())
        };
        self.cache_dirty
            .store(persisted.is_err(), Ordering::Release);
        // Memory still honors revocations when persistence fails. Remove the
        // old cache so it cannot resurrect a revoked entry after a restart.
        if persisted.is_err() {
            match tokio::fs::remove_file(&self.cache_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!(operation = "node.serve.certificates.cache_remove_failed", %error, "failed to invalidate certificate cache after persistence failure")
                }
            }
        }
        *self.snapshot.write().expect("ingress state poisoned") = Snapshot {
            certificates,
            challenges,
            challenge_revision,
        };
        persisted
    }

    pub fn status(&self) -> ReportIngressStatusRequest {
        let snapshot = self.snapshot.read().expect("ingress state poisoned");
        let now = unix_now();
        let tls_ready = self.tls_enabled && self.tls_ready.load(Ordering::Acquire);
        let mut certificates = if tls_ready {
            snapshot
                .certificates
                .values()
                .filter(|certificate| certificate.valid_at(now))
                .map(|certificate| InstalledCertificate {
                    ingress_id: certificate.bundle.ingress_id,
                    revision: certificate.bundle.revision.clone(),
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        certificates.sort_by_key(|certificate| certificate.ingress_id);
        ReportIngressStatusRequest {
            certificates,
            challenge_revision: if self.http_ready.load(Ordering::Acquire)
                && snapshot
                    .challenges
                    .values()
                    .all(|challenge| challenge.expires_at_unix > now)
            {
                snapshot.challenge_revision.clone()
            } else {
                String::new()
            },
            tls_ready,
        }
    }

    pub fn listeners_ready(&self) -> bool {
        self.http_ready.load(Ordering::Acquire)
            && (!self.tls_enabled || self.tls_ready.load(Ordering::Acquire))
    }

    pub fn certificate_for(&self, hostname: &str) -> Option<Arc<CertifiedKey>> {
        self.snapshot
            .read()
            .expect("ingress state poisoned")
            .certificates
            .get(hostname)
            .filter(|certificate| certificate.valid_at(unix_now()))
            .map(|certificate| certificate.key.clone())
    }

    pub fn challenge(&self, hostname: &str, token: &str) -> Option<String> {
        if !safe_token(token) {
            return None;
        }
        self.snapshot
            .read()
            .expect("ingress state poisoned")
            .challenges
            .get(&(hostname.to_owned(), token.to_owned()))
            .filter(|challenge| challenge.expires_at_unix > unix_now())
            .map(|challenge| challenge.key_authorization.clone())
    }
}

impl ResolvesServerCert for IngressState {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let hostname = safe_hostname(hello.server_name()?).ok()?;
        self.certificate_for(&hostname)
    }
}

fn validate_bundle(mut bundle: CertificateBundle, now: i64) -> anyhow::Result<LoadedCertificate> {
    bundle.hostname = safe_hostname(&bundle.hostname)?;
    if bundle.certificate_pem.len() > MAX_CERTIFICATE_BYTES
        || bundle.private_key_pem.len() > MAX_PRIVATE_KEY_BYTES
    {
        anyhow::bail!("certificate bundle exceeds the size limit");
    }
    let revision = hex::encode(Sha256::digest(bundle.certificate_pem.as_bytes()));
    if !bundle.revision.is_empty() && bundle.revision != revision {
        anyhow::bail!("certificate revision does not match certificate contents");
    }
    bundle.revision = revision;
    let certificates = rustls_pemfile::certs(&mut Cursor::new(bundle.certificate_pem.as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .context("invalid certificate PEM")?;
    if certificates.is_empty() || certificates.len() > 16 {
        anyhow::bail!("invalid certificate chain length");
    }
    let private_key =
        rustls_pemfile::private_key(&mut Cursor::new(bundle.private_key_pem.as_bytes()))
            .context("invalid private key PEM")?
            .context("certificate private key is missing")?;
    let parsed = rustls::server::ParsedCertificate::try_from(&certificates[0])
        .context("invalid leaf certificate")?;
    let name =
        ServerName::try_from(bundle.hostname.as_str()).context("invalid certificate DNS name")?;
    rustls::client::verify_server_name(&parsed, &name)
        .context("certificate does not cover its hostname")?;
    let mut not_before = i64::MIN;
    let mut not_after = i64::MAX;
    for certificate in &certificates {
        let (_, parsed) = x509_parser::parse_x509_certificate(certificate.as_ref())
            .map_err(|_| anyhow::anyhow!("invalid certificate DER"))?;
        not_before = not_before.max(parsed.validity().not_before.timestamp());
        not_after = not_after.min(parsed.validity().not_after.timestamp());
    }
    if now < not_before || now >= not_after {
        anyhow::bail!("certificate chain is not currently valid");
    }
    let key = CertifiedKey::from_der(
        certificates,
        private_key,
        &rustls::crypto::ring::default_provider(),
    )
    .context("certificate private key does not match")?;
    key.keys_match()
        .context("certificate key match cannot be verified")?;
    bundle.issued_at_unix = Some(not_before);
    bundle.expires_at_unix = Some(not_after);
    Ok(LoadedCertificate {
        bundle,
        key: Arc::new(key),
        not_before,
        not_after,
    })
}

type ChallengeMap = HashMap<(String, String), HttpChallenge>;

fn validate_challenges(
    challenges: Vec<HttpChallenge>,
    revision: String,
    now: i64,
) -> anyhow::Result<(ChallengeMap, String)> {
    if revision.len() > 128
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || (!challenges.is_empty() && revision.is_empty())
    {
        anyhow::bail!("invalid HTTP challenge revision");
    }
    let mut accepted = HashMap::new();
    for mut challenge in challenges {
        challenge.hostname = safe_hostname(&challenge.hostname)?;
        let thumbprint = challenge
            .key_authorization
            .strip_prefix(&format!("{}.", challenge.token));
        if !safe_token(&challenge.token)
            || !thumbprint.is_some_and(|value| value.len() == 43 && safe_token(value))
            || challenge.expires_at_unix <= now
            || challenge.expires_at_unix > now.saturating_add(86400)
        {
            anyhow::bail!("invalid or expired HTTP challenge");
        }
        if accepted
            .insert(
                (challenge.hostname.clone(), challenge.token.clone()),
                challenge,
            )
            .is_some()
        {
            anyhow::bail!("duplicate HTTP challenge");
        }
    }
    Ok((accepted, revision))
}

fn safe_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 256
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn safe_hostname(hostname: &str) -> anyhow::Result<String> {
    let normalized = grass_validator::normalize_host(hostname)
        .map_err(|_| anyhow::anyhow!("invalid ingress hostname"))?;
    if normalized.len() > 253
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        anyhow::bail!("ingress hostname contains unsafe characters");
    }
    Ok(normalized)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .try_into()
        .unwrap_or(i64::MAX)
}

async fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let directory = path.parent().context("certificate cache has no parent")?;
    tokio::fs::create_dir_all(directory).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let temporary = directory.join(format!("active-{}.tmp", Uuid::now_v7().simple()));
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temporary, path).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

async fn refresh(client: &ControlApiClient, ingress: &IngressState) -> anyhow::Result<()> {
    match client.certificate_bundles().await {
        Ok(response) => ingress.apply(response).await?,
        Err(RouteSnapshotError::AuthorizationRevoked) => {
            ingress.apply(CertificateBundlesResponse::default()).await?;
            anyhow::bail!("ingress authorization has been revoked");
        }
        Err(error) => return Err(error.into()),
    }
    client.report_ingress_status(&ingress.status()).await
}

pub fn spawn(client: ControlApiClient, ingress: Arc<IngressState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = refresh(&client, &ingress).await {
                warn!(operation = "node.serve.certificates.refresh_failed", %error, "failed to synchronize ingress state");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::State,
        http::StatusCode,
        routing::{get, post},
    };
    use grass_node_protocol::{RouteSnapshotResponse, ServeAccess, ServeResources, ServeRoute};
    use rcgen::{CertificateParams, KeyPair};
    use tokio::net::TcpListener;

    use super::*;
    use crate::serve::{
        ResolvedTarget, ServeState, routes::RouteTable, serve_router, ssr::SsrManager, tls,
    };

    fn config(root: &Path) -> NodeConfig {
        let mut config = NodeConfig::default();
        config.serve.artifact_cache_root = root.display().to_string();
        config.serve.tls.enabled = true;
        config
    }

    fn bundle(host: &str, not_before: i64, not_after: i64) -> CertificateBundle {
        let mut parameters = CertificateParams::new(vec![host.to_owned()]).unwrap();
        parameters.not_before = time::OffsetDateTime::from_unix_timestamp(not_before).unwrap();
        parameters.not_after = time::OffsetDateTime::from_unix_timestamp(not_after).unwrap();
        let key = KeyPair::generate().unwrap();
        let certificate = parameters.self_signed(&key).unwrap();
        let pem = certificate.pem();
        CertificateBundle {
            ingress_id: Uuid::now_v7(),
            hostname: host.to_owned(),
            revision: hex::encode(Sha256::digest(pem.as_bytes())),
            certificate_pem: pem,
            private_key_pem: key.serialize_pem(),
            // Deliberately false metadata: validation must use certificate DER.
            issued_at_unix: Some(0),
            expires_at_unix: Some(i64::MAX),
        }
    }

    fn valid_bundle(host: &str) -> CertificateBundle {
        let now = unix_now();
        bundle(host, now - 60, now + 3600)
    }

    fn snapshot(bundles: Vec<CertificateBundle>) -> CertificateBundlesResponse {
        CertificateBundlesResponse {
            bundles,
            ..Default::default()
        }
    }

    fn challenge(host: &str) -> HttpChallenge {
        HttpChallenge {
            hostname: host.to_owned(),
            token: "test-token_123".to_owned(),
            key_authorization: format!("test-token_123.{}", "A".repeat(43)),
            expires_at_unix: unix_now() + 300,
        }
    }

    #[test]
    fn certificate_validation_checks_real_validity_names_key_and_revision() {
        let now = unix_now();
        let original = bundle("edge.example.test", now - 60, now + 300);
        let loaded = validate_bundle(original.clone(), now).unwrap();
        assert_eq!(loaded.bundle.expires_at_unix, Some(now + 300));
        assert_eq!(loaded.bundle.issued_at_unix, Some(now - 60));
        assert!(!format!("{original:?}").contains("PRIVATE KEY"));
        assert!(!format!("{original:?}").contains(&original.private_key_pem));

        let mut wrong_name = original.clone();
        wrong_name.hostname = "another.example.test".to_owned();
        assert!(validate_bundle(wrong_name, now).is_err());
        let mut wrong_key = original.clone();
        wrong_key.private_key_pem = KeyPair::generate().unwrap().serialize_pem();
        assert!(validate_bundle(wrong_key, now).is_err());
        let mut wrong_revision = original.clone();
        wrong_revision.revision = "a".repeat(64);
        assert!(validate_bundle(wrong_revision, now).is_err());
        assert!(validate_bundle(bundle("edge.example.test", now - 120, now - 1), now).is_err());
        assert!(validate_bundle(bundle("edge.example.test", now + 60, now + 300), now).is_err());
        assert!(validate_bundle(original, now + 300).is_err());
        assert!(safe_hostname("../secret").is_err());
    }

    #[tokio::test]
    async fn invalid_renewal_retains_only_the_same_valid_certificate_identity() {
        let root = tempfile::tempdir().unwrap();
        let state = IngressState::new(&config(root.path()), Uuid::now_v7());
        state.tls_ready.store(true, Ordering::Release);
        let original = valid_bundle("edge.example.test");
        state.apply(snapshot(vec![original.clone()])).await.unwrap();
        let mut invalid = original.clone();
        invalid.certificate_pem = "invalid PEM".to_owned();
        invalid.revision = "bad".to_owned();
        state.apply(snapshot(vec![invalid.clone()])).await.unwrap();
        assert_eq!(state.status().certificates[0].revision, original.revision);

        // Reassignment of the hostname to a different certificate owner cannot
        // accidentally keep the previous owner's certificate live.
        invalid.ingress_id = Uuid::now_v7();
        state.apply(snapshot(vec![invalid])).await.unwrap();
        assert!(state.certificate_for("edge.example.test").is_none());
        assert!(state.status().certificates.is_empty());
    }

    #[tokio::test]
    async fn expired_certificates_cannot_be_selected_or_retained_on_failed_renewal() {
        let root = tempfile::tempdir().unwrap();
        let state = IngressState::new(&config(root.path()), Uuid::now_v7());
        state.tls_ready.store(true, Ordering::Release);
        let now = unix_now();
        let expired = bundle("expired.example.test", now - 300, now - 1);
        state
            .apply_at(snapshot(vec![expired.clone()]), now - 120)
            .await
            .unwrap();
        assert!(state.certificate_for("expired.example.test").is_none());
        assert!(state.status().certificates.is_empty());
        let mut invalid = expired;
        invalid.private_key_pem = "broken".to_owned();
        state.apply(snapshot(vec![invalid])).await.unwrap();
        assert!(state.snapshot.read().unwrap().certificates.is_empty());
    }

    #[tokio::test]
    async fn cache_restarts_preserve_valid_certificates_without_resurrecting_withdrawals() {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let node_id = Uuid::now_v7();
        let state = IngressState::new(&config, node_id);
        state
            .apply(snapshot(vec![valid_bundle("edge.example.test")]))
            .await
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                tokio::fs::metadata(&state.cache_path)
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                tokio::fs::metadata(state.cache_path.parent().unwrap())
                    .await
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let restored = IngressState::new(&config, node_id);
        restored.restore().await.unwrap();
        assert!(restored.certificate_for("edge.example.test").is_some());
        let different_node = IngressState::new(&config, Uuid::now_v7());
        different_node.restore().await.unwrap();
        assert!(
            different_node
                .certificate_for("edge.example.test")
                .is_none()
        );
        let mut different_region_config = config.clone();
        different_region_config.node.region = "another".to_owned();
        let different_region = IngressState::new(&different_region_config, node_id);
        different_region.restore().await.unwrap();
        assert!(
            different_region
                .certificate_for("edge.example.test")
                .is_none()
        );

        state.apply(snapshot(Vec::new())).await.unwrap();
        let after_withdrawal = IngressState::new(&config, node_id);
        after_withdrawal.restore().await.unwrap();
        assert!(
            after_withdrawal
                .certificate_for("edge.example.test")
                .is_none()
        );
        assert!(
            !tokio::fs::read_to_string(&state.cache_path)
                .await
                .unwrap()
                .contains("PRIVATE KEY")
        );

        // A cached certificate that expired while the Node was offline is not
        // revived, even if supplied metadata still claims distant expiry.
        let now = unix_now();
        state
            .apply_at(
                snapshot(vec![bundle("edge.example.test", now - 300, now - 1)]),
                now - 120,
            )
            .await
            .unwrap();
        let expired_restart = IngressState::new(&config, node_id);
        expired_restart.restore().await.unwrap();
        assert!(
            expired_restart
                .certificate_for("edge.example.test")
                .is_none()
        );
    }

    #[tokio::test]
    async fn challenge_snapshots_are_exact_scoped_expiring_and_acknowledged_only_when_serving() {
        let root = tempfile::tempdir().unwrap();
        let state = IngressState::new(&config(root.path()), Uuid::now_v7());
        let original = challenge("Edge.Example.test");
        let response = CertificateBundlesResponse {
            challenges: vec![original.clone()],
            challenge_revision: "revision-1".to_owned(),
            ..Default::default()
        };
        state.apply(response.clone()).await.unwrap();
        assert!(state.status().challenge_revision.is_empty());
        state.http_ready.store(true, Ordering::Release);
        assert_eq!(state.status().challenge_revision, "revision-1");
        assert_eq!(
            state.challenge("edge.example.test", &original.token),
            Some(original.key_authorization)
        );
        assert!(
            state
                .challenge("other.example.test", &original.token)
                .is_none()
        );
        for token in [
            "../test-token_123",
            "test-token_123/extra",
            "%74est-token_123",
            "wrong",
        ] {
            assert!(state.challenge("edge.example.test", token).is_none());
        }

        let mut expired = response.clone();
        expired.challenges[0].expires_at_unix = unix_now() - 1;
        state.apply(expired).await.unwrap();
        assert!(
            state
                .challenge("edge.example.test", "test-token_123")
                .is_none()
        );
        assert!(state.status().challenge_revision.is_empty());
        state.apply(response).await.unwrap();
        state
            .apply(CertificateBundlesResponse {
                challenge_revision: "revision-empty".to_owned(),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            state
                .challenge("edge.example.test", "test-token_123")
                .is_none()
        );
        assert_eq!(state.status().challenge_revision, "revision-empty");
    }

    async fn serve_state(root: &Path) -> Arc<ServeState> {
        let config = config(root);
        let node_id = Uuid::now_v7();
        let routes = Arc::new(RouteTable::default());
        routes
            .apply(RouteSnapshotResponse {
                revision: "ready".to_owned(),
                routes: Vec::new(),
            })
            .await
            .unwrap();
        let client = ControlApiClient::new("http://127.0.0.1:1", "test-token").unwrap();
        let ssr = Arc::new(SsrManager::with_client(
            None,
            node_id,
            &config,
            client.clone(),
        ));
        let state = Arc::new(ServeState::new(
            client,
            node_id,
            "gateway-test-token".to_owned(),
            routes,
            &config,
            ssr,
        ));
        state.ingress.http_ready.store(true, Ordering::Release);
        state.ingress.tls_ready.store(true, Ordering::Release);
        state
    }

    struct ServerTask(tokio::task::JoinHandle<()>);

    impl Drop for ServerTask {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    fn https_client(
        address: std::net::SocketAddr,
        bundles: &[CertificateBundle],
    ) -> reqwest::Client {
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .tls_info(true)
            .http1_only()
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(3));
        for bundle in bundles {
            builder = builder
                .add_root_certificate(
                    reqwest::Certificate::from_pem(bundle.certificate_pem.as_bytes()).unwrap(),
                )
                .resolve(&bundle.hostname, address);
        }
        builder
            .resolve("unknown.example.test", address)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn real_https_serves_sites_rejects_sni_host_confusion_and_hot_swaps_certificates() {
        let root = tempfile::tempdir().unwrap();
        let state = serve_state(root.path()).await;
        let first = valid_bundle("a.example.test");
        let second_host = valid_bundle("b.example.test");
        state
            .ingress
            .apply(snapshot(vec![first.clone(), second_host.clone()]))
            .await
            .unwrap();
        let mut routes = Vec::new();
        for (host, text) in [
            ("a.example.test", "tenant A"),
            ("b.example.test", "tenant B"),
        ] {
            let deployment_id = Uuid::now_v7();
            let directory = root.path().join(host);
            tokio::fs::create_dir(&directory).await.unwrap();
            tokio::fs::write(directory.join("index.html"), text)
                .await
                .unwrap();
            state.targets.lock().await.insert(
                deployment_id,
                ResolvedTarget::Static {
                    static_dir: directory,
                    spa_fallback: false,
                    not_found: None,
                },
            );
            routes.push(ServeRoute {
                host: host.to_owned(),
                region: "default".to_owned(),
                deployment_id,
                target_node_id: state.node_id,
                target_base_url: "http://127.0.0.1:1".to_owned(),
                resources: ServeResources {
                    cpu_millicores: 50,
                    memory_mb: 64,
                    disk_mb: 128,
                },
                access: ServeAccess::Public,
            });
        }
        state
            .routes
            .apply(RouteSnapshotResponse {
                revision: "sites".to_owned(),
                routes,
            })
            .await
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = serve_router(state.clone());
        let ingress = state.ingress.clone();
        let _server = ServerTask(tokio::spawn(async move {
            tls::serve(listener, app, ingress).await.unwrap();
        }));
        let client = https_client(address, &[first.clone(), second_host.clone()]);
        let response = client
            .get(format!("https://a.example.test:{}/", address.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let first_der = response
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .unwrap()
            .peer_certificate()
            .unwrap()
            .to_vec();
        assert_eq!(response.text().await.unwrap(), "tenant A");
        let mismatched = client
            .get(format!("https://a.example.test:{}/", address.port()))
            .header("host", "b.example.test")
            .send()
            .await
            .unwrap();
        assert_eq!(mismatched.status(), StatusCode::MISDIRECTED_REQUEST);
        assert!(
            client
                .get(format!("https://unknown.example.test:{}/", address.port()))
                .send()
                .await
                .is_err()
        );

        let mut renewed = valid_bundle("a.example.test");
        renewed.ingress_id = first.ingress_id;
        state
            .ingress
            .apply(snapshot(vec![renewed.clone(), second_host.clone()]))
            .await
            .unwrap();
        let renewed_client = https_client(address, &[renewed.clone(), second_host.clone()]);
        let response = renewed_client
            .get(format!("https://a.example.test:{}/", address.port()))
            .send()
            .await
            .unwrap();
        let renewed_der = response
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .unwrap()
            .peer_certificate()
            .unwrap();
        assert_ne!(renewed_der, first_der);
        assert_eq!(response.text().await.unwrap(), "tenant A");
        assert!(
            state
                .ingress
                .status()
                .certificates
                .iter()
                .any(|certificate| certificate.revision == renewed.revision)
        );

        let mut broken = renewed.clone();
        broken.private_key_pem = "bad private key".to_owned();
        state
            .ingress
            .apply(snapshot(vec![broken, second_host.clone()]))
            .await
            .unwrap();
        let preserved = renewed_client
            .get(format!("https://a.example.test:{}/", address.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(preserved.status(), StatusCode::OK);
        assert_eq!(
            state
                .ingress
                .status()
                .certificates
                .iter()
                .find(|certificate| certificate.ingress_id == first.ingress_id)
                .unwrap()
                .revision,
            renewed.revision
        );

        state
            .ingress
            .apply(snapshot(vec![second_host]))
            .await
            .unwrap();
        assert!(
            renewed_client
                .get(format!("https://a.example.test:{}/", address.port()))
                .send()
                .await
                .is_err()
        );
        let still_serving = renewed_client
            .get(format!("https://b.example.test:{}/", address.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(still_serving.text().await.unwrap(), "tenant B");
    }

    #[tokio::test]
    async fn http_health_and_challenges_work_without_a_deployment_and_never_bypass_other_hosts() {
        let root = tempfile::tempdir().unwrap();
        let state = serve_state(root.path()).await;
        let challenge = challenge("edge.example.test");
        state
            .ingress
            .apply(CertificateBundlesResponse {
                challenges: vec![challenge.clone()],
                challenge_revision: "revision-1".to_owned(),
                ..Default::default()
            })
            .await
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = serve_router(state.clone());
        let _server = ServerTask(tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        }));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://{address}");
        let health = client
            .get(format!("{base}/_grass/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
        state.ingress.tls_ready.store(false, Ordering::Release);
        assert_eq!(
            client
                .get(format!("{base}/_grass/health"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let challenge_url = format!("{base}{CHALLENGE_PREFIX}{}", challenge.token);
        let response = client
            .get(&challenge_url)
            .header("host", "EDGE.EXAMPLE.TEST:80")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), challenge.key_authorization);
        assert_eq!(
            client
                .get(&challenge_url)
                .header("host", "another.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            client
                .post(&challenge_url)
                .header("host", "edge.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            client
                .get(format!("{base}/"))
                .header("host", "edge.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            client
                .get(format!("{base}{CHALLENGE_PREFIX}%74est-token_123"))
                .header("host", "edge.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        state
            .ingress
            .apply(CertificateBundlesResponse::default())
            .await
            .unwrap();
        assert_eq!(
            client
                .get(&challenge_url)
                .header("host", "edge.example.test")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[derive(Clone)]
    struct MockControl {
        response: Arc<tokio::sync::RwLock<(StatusCode, CertificateBundlesResponse)>>,
        reports: Arc<tokio::sync::Mutex<Vec<ReportIngressStatusRequest>>>,
    }

    async fn control_snapshot(
        State(state): State<MockControl>,
    ) -> impl axum::response::IntoResponse {
        let response = state.response.read().await;
        (
            response.0,
            Json(
                serde_json::json!({ "code": response.0.as_u16(), "message": "snapshot", "data": response.1 }),
            ),
        )
    }

    async fn control_report(
        State(state): State<MockControl>,
        Json(report): Json<ReportIngressStatusRequest>,
    ) -> Json<serde_json::Value> {
        state.reports.lock().await.push(report);
        Json(serde_json::json!({ "code": 200, "message": "ok", "data": { "acknowledged": true } }))
    }

    #[tokio::test]
    async fn synchronization_preserves_outages_distinguishes_revocation_and_reports_loaded_state() {
        let root = tempfile::tempdir().unwrap();
        let ingress = IngressState::new(&config(root.path()), Uuid::now_v7());
        ingress.http_ready.store(true, Ordering::Release);
        ingress.tls_ready.store(true, Ordering::Release);
        let control = MockControl {
            response: Arc::new(tokio::sync::RwLock::new((
                StatusCode::OK,
                CertificateBundlesResponse {
                    bundles: vec![valid_bundle("edge.example.test")],
                    challenges: vec![challenge("edge.example.test")],
                    challenge_revision: "revision-1".to_owned(),
                },
            ))),
            reports: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ControlApiClient::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            "node-test-token",
        )
        .unwrap();
        let app = Router::new()
            .route("/api/v1/internal/serve/certificates", get(control_snapshot))
            .route(
                "/api/v1/internal/serve/ingress-status",
                post(control_report),
            )
            .with_state(control.clone());
        let _server = ServerTask(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }));
        refresh(&client, &ingress).await.unwrap();
        let reports = control.reports.lock().await;
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].certificates.len(), 1);
        assert_eq!(reports[0].challenge_revision, "revision-1");
        assert!(reports[0].tls_ready);
        drop(reports);
        control.response.write().await.0 = StatusCode::SERVICE_UNAVAILABLE;
        assert!(refresh(&client, &ingress).await.is_err());
        assert!(ingress.certificate_for("edge.example.test").is_some());
        assert!(
            ingress
                .challenge("edge.example.test", "test-token_123")
                .is_some()
        );
        assert_eq!(control.reports.lock().await.len(), 1);
        control.response.write().await.0 = StatusCode::UNAUTHORIZED;
        assert!(refresh(&client, &ingress).await.is_err());
        assert!(ingress.certificate_for("edge.example.test").is_none());
        assert!(
            ingress
                .challenge("edge.example.test", "test-token_123")
                .is_none()
        );
        let restarted = IngressState::new(&config(root.path()), ingress.node_id);
        restarted.restore().await.unwrap();
        assert!(restarted.certificate_for("edge.example.test").is_none());
    }
}
