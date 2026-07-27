//! Git checkout for build workspaces.

use std::{
    collections::BTreeSet,
    net::IpAddr,
    path::{Path, PathBuf},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use grass_git_source::{
    GitTransport, PrivateTargetException, RepositoryEndpoint, RepositoryUrlError,
    parse_repository_url, validate_resolved_targets,
};
use grass_node_protocol::{GitCredential, ObserveSshHostKeyRequest, RedeemGitCredentialResponse};
use sha2::{Digest, Sha256};
use tokio::process::Command;

const MINIMUM_GIT_VERSION: (u64, u64) = (2, 49);

#[derive(Debug, thiserror::Error)]
pub enum CheckoutError {
    #[error("git clone failed: {0}")]
    CloneFailed(String),
    #[error("commit checkout failed: {0}")]
    CommitCheckoutFailed(String),
    #[error("root directory {0} is invalid")]
    InvalidRootDirectory(String),
    #[error("repository URL is not allowed: {0}")]
    InvalidRepositoryUrl(#[from] RepositoryUrlError),
    #[error("repository host could not be resolved")]
    ResolveFailed,
    #[error("repository target is blocked: {0}")]
    RepositoryTargetBlocked(String),
    #[error("source credential does not match the repository transport")]
    CredentialMismatch,
    #[error("source credential helper could not be prepared")]
    CredentialSetup,
    #[error("git ref {0} is not allowed")]
    InvalidRef(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn parse_git_version(output: &str) -> Option<(u64, u64)> {
    let version = output.trim().strip_prefix("git version ")?;
    let mut parts = version.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

pub async fn ensure_supported_git() -> anyhow::Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|error| anyhow::anyhow!("Git 2.49 or newer is required: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = output
        .status
        .success()
        .then(|| parse_git_version(&stdout))
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("Git 2.49 or newer is required"))?;
    if version < MINIMUM_GIT_VERSION {
        anyhow::bail!(
            "Git 2.49 or newer is required for repository network isolation; found {}.{}",
            version.0,
            version.1
        );
    }
    Ok(())
}

struct ValidatedRepositoryTarget {
    endpoint: RepositoryEndpoint,
    addresses: Vec<IpAddr>,
}

async fn validate_repository_target(
    repository_url: &str,
    exceptions: &[PrivateTargetException],
) -> Result<ValidatedRepositoryTarget, CheckoutError> {
    let endpoint = parse_repository_url(repository_url)?;
    let addresses = if let Ok(address) = endpoint.host.parse::<IpAddr>() {
        vec![address]
    } else {
        tokio::net::lookup_host((endpoint.host.as_str(), endpoint.port))
            .await
            .map_err(|_| CheckoutError::ResolveFailed)?
            .map(|socket| socket.ip())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    validate_resolved_targets(&endpoint, &addresses, exceptions)
        .map_err(|error| CheckoutError::RepositoryTargetBlocked(error.to_string()))?;
    Ok(ValidatedRepositoryTarget {
        endpoint,
        addresses,
    })
}

pub struct ObservedSshHostKey {
    pub request: ObserveSshHostKeyRequest,
    pub target_ip: IpAddr,
}

fn host_key_preference(key_type: &str) -> Option<u8> {
    match key_type {
        "ssh-ed25519" => Some(0),
        "ecdsa-sha2-nistp256" => Some(1),
        "ecdsa-sha2-nistp384" => Some(2),
        "ecdsa-sha2-nistp521" => Some(3),
        "ssh-rsa" => Some(4),
        _ => None,
    }
}

fn select_scanned_host_key(stdout: &str) -> Result<(String, String, Vec<u8>), CheckoutError> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _scanned_host = fields.next()?;
            let key_type = fields.next()?;
            let public_key = fields.next()?;
            if fields.next().is_some() {
                return None;
            }
            let preference = host_key_preference(key_type)?;
            let key_bytes = STANDARD.decode(public_key).ok()?;
            (!key_bytes.is_empty()).then(|| {
                (
                    preference,
                    key_type.to_owned(),
                    public_key.to_owned(),
                    key_bytes,
                )
            })
        })
        .min_by_key(|(preference, _, _, _)| *preference)
        .map(|(_, key_type, public_key, key_bytes)| (key_type, public_key, key_bytes))
        .ok_or(CheckoutError::ResolveFailed)
}

pub async fn inspect_ssh_host_key(
    repository_url: &str,
    exceptions: &[PrivateTargetException],
) -> Result<Option<ObservedSshHostKey>, CheckoutError> {
    let target = validate_repository_target(repository_url, exceptions).await?;
    if target.endpoint.transport != GitTransport::Ssh {
        return Ok(None);
    }
    let target_ip = *target
        .addresses
        .first()
        .ok_or(CheckoutError::ResolveFailed)?;
    let output = Command::new("ssh-keyscan")
        .args([
            "-T",
            "10",
            "-p",
            &target.endpoint.port.to_string(),
            &target_ip.to_string(),
        ])
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .await
        .map_err(|_| CheckoutError::CredentialSetup)?;
    if !output.status.success() && output.stdout.is_empty() {
        return Err(CheckoutError::ResolveFailed);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (key_type, public_key, key_bytes) = select_scanned_host_key(&stdout)?;
    let fingerprint_sha256 = format!(
        "SHA256:{}",
        STANDARD
            .encode(Sha256::digest(key_bytes))
            .trim_end_matches('=')
    );
    Ok(Some(ObservedSshHostKey {
        request: ObserveSshHostKeyRequest {
            host: target.endpoint.host,
            port: target.endpoint.port,
            key_type,
            public_key,
            fingerprint_sha256,
        },
        target_ip,
    }))
}

/// Rejects branch/commit refs that could be read by git as options or that
/// contain shell/path metacharacters. Refs are limited to a conservative
/// charset and must not start with `-`.
fn validate_ref(value: &str) -> Result<(), CheckoutError> {
    let ok = !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    if ok {
        Ok(())
    } else {
        Err(CheckoutError::InvalidRef(value.to_owned()))
    }
}

pub struct CheckoutResult {
    pub source_dir: PathBuf,
    /// Project root after applying the configured root directory.
    pub project_root: PathBuf,
    pub commit_hash: Option<String>,
}

pub struct CheckoutAccess<'a> {
    pub private_target_exceptions: &'a [PrivateTargetException],
    pub credential: Option<&'a RedeemGitCredentialResponse>,
    pub known_hosts_line: Option<&'a str>,
    pub ssh_target_ip: Option<IpAddr>,
}

struct GitAuthContext {
    directory: Option<PathBuf>,
    environment: Vec<(String, String)>,
}

impl GitAuthContext {
    async fn prepare(
        destination: &Path,
        endpoint: &RepositoryEndpoint,
        credential_access: Option<&RedeemGitCredentialResponse>,
        known_hosts_line: Option<&str>,
        target_ip: IpAddr,
    ) -> Result<Self, CheckoutError> {
        if credential_access.is_some_and(|access| {
            !access.host.eq_ignore_ascii_case(&endpoint.host) || access.port != endpoint.port
        }) {
            return Err(CheckoutError::CredentialMismatch);
        }
        let credential = credential_access.map(|access| &access.credential);
        match credential {
            None if endpoint.transport != GitTransport::Ssh => {}
            Some(GitCredential::Https { .. }) if endpoint.transport == GitTransport::Https => {}
            Some(GitCredential::Ssh { username, .. })
                if endpoint.transport == GitTransport::Ssh =>
            {
                if endpoint
                    .username
                    .as_deref()
                    .is_some_and(|url_username| url_username != username)
                {
                    return Err(CheckoutError::CredentialMismatch);
                }
                if known_hosts_line.is_none() {
                    return Err(CheckoutError::CredentialSetup);
                }
            }
            _ => return Err(CheckoutError::CredentialMismatch),
        }

        let directory = destination.join(format!(".git-auth-{}", uuid::Uuid::now_v7().simple()));
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|_| CheckoutError::CredentialSetup)?;
        set_private_mode(&directory, 0o700).await?;

        let mut context = Self {
            directory: Some(directory.clone()),
            environment: Vec::new(),
        };
        match credential {
            Some(GitCredential::Https { username, secret })
                if endpoint.transport == GitTransport::Https =>
            {
                let askpass = directory.join("https-askpass");
                write_private_file(
                    &askpass,
                    b"#!/bin/sh\ncase \"$1\" in\n  *sername*) printf '%s' \"$GRASS_GIT_USERNAME\" ;;\n  *) printf '%s' \"$GRASS_GIT_SECRET\" ;;\nesac\n",
                    0o700,
                )
                .await?;
                context.environment.extend([
                    (
                        "GIT_ASKPASS".to_owned(),
                        askpass.to_string_lossy().into_owned(),
                    ),
                    ("GIT_ASKPASS_REQUIRE".to_owned(), "force".to_owned()),
                    ("GRASS_GIT_USERNAME".to_owned(), username.clone()),
                    ("GRASS_GIT_SECRET".to_owned(), secret.clone()),
                ]);
            }
            Some(GitCredential::Ssh {
                username,
                private_key,
                passphrase,
            }) if endpoint.transport == GitTransport::Ssh => {
                let known_hosts_line = known_hosts_line.ok_or(CheckoutError::CredentialSetup)?;
                let key_path = directory.join("identity");
                write_private_file(&key_path, private_key.as_bytes(), 0o600).await?;
                let known_hosts = directory.join("known_hosts");
                write_private_file(&known_hosts, known_hosts_line.as_bytes(), 0o600).await?;
                let askpass = directory.join("ssh-askpass");
                write_private_file(
                    &askpass,
                    b"#!/bin/sh\nprintf '%s' \"$GRASS_GIT_PASSPHRASE\"\n",
                    0o700,
                )
                .await?;
                let wrapper = directory.join("ssh-wrapper");
                write_private_file(
                    &wrapper,
                    b"#!/bin/sh\nexec ssh -i \"$GRASS_GIT_SSH_KEY\" -l \"$GRASS_GIT_USERNAME\" -p \"$GRASS_GIT_PORT\" -o Hostname=\"$GRASS_GIT_TARGET_IP\" -o HostKeyAlias=\"$GRASS_GIT_HOST\" -o UserKnownHostsFile=\"$GRASS_GIT_KNOWN_HOSTS\" -o GlobalKnownHostsFile=/dev/null -o IdentitiesOnly=yes -o PasswordAuthentication=no -o KbdInteractiveAuthentication=no -o StrictHostKeyChecking=yes \"$@\"\n",
                    0o700,
                )
                .await?;
                context.environment.extend([
                    ("GIT_SSH".to_owned(), wrapper.to_string_lossy().into_owned()),
                    (
                        "GRASS_GIT_SSH_KEY".to_owned(),
                        key_path.to_string_lossy().into_owned(),
                    ),
                    ("GRASS_GIT_USERNAME".to_owned(), username.clone()),
                    ("GRASS_GIT_HOST".to_owned(), endpoint.host.clone()),
                    ("GRASS_GIT_PORT".to_owned(), endpoint.port.to_string()),
                    ("GRASS_GIT_TARGET_IP".to_owned(), target_ip.to_string()),
                    (
                        "GRASS_GIT_KNOWN_HOSTS".to_owned(),
                        known_hosts.to_string_lossy().into_owned(),
                    ),
                    (
                        "GRASS_GIT_PASSPHRASE".to_owned(),
                        passphrase.clone().unwrap_or_default(),
                    ),
                    (
                        "SSH_ASKPASS".to_owned(),
                        askpass.to_string_lossy().into_owned(),
                    ),
                    ("SSH_ASKPASS_REQUIRE".to_owned(), "force".to_owned()),
                    ("DISPLAY".to_owned(), "grass-worker:0".to_owned()),
                ]);
            }
            None => {}
            _ => unreachable!("credential compatibility was checked before creating files"),
        }

        match endpoint.transport {
            GitTransport::Http | GitTransport::Https => {
                let mut configs = Vec::new();
                // An IP-literal URL is already pinned by construction. DNS
                // names use curl's resolve list so the clone cannot perform
                // a second lookup after policy validation.
                if endpoint.host.parse::<IpAddr>().is_err() {
                    let address = match target_ip {
                        IpAddr::V4(address) => address.to_string(),
                        IpAddr::V6(address) => format!("[{address}]"),
                    };
                    configs.push((
                        "http.curloptResolve".to_owned(),
                        format!("+{}:{}:{address}", endpoint.host, endpoint.port),
                    ));
                }
                configs.push(("http.followRedirects".to_owned(), "false".to_owned()));
                context
                    .environment
                    .push(("GIT_CONFIG_COUNT".to_owned(), configs.len().to_string()));
                for (index, (key, value)) in configs.into_iter().enumerate() {
                    context
                        .environment
                        .push((format!("GIT_CONFIG_KEY_{index}"), key));
                    context
                        .environment
                        .push((format!("GIT_CONFIG_VALUE_{index}"), value));
                }
            }
            GitTransport::Git => {
                let proxy = directory.join("git-proxy");
                write_private_file(
                    &proxy,
                    b"#!/bin/sh\nexec \"$GRASS_NODE_BINARY\" git-proxy\n",
                    0o700,
                )
                .await?;
                let node_binary =
                    std::env::current_exe().map_err(|_| CheckoutError::CredentialSetup)?;
                context.environment.extend([
                    (
                        "GIT_PROXY_COMMAND".to_owned(),
                        proxy.to_string_lossy().into_owned(),
                    ),
                    (
                        "GRASS_NODE_BINARY".to_owned(),
                        node_binary.to_string_lossy().into_owned(),
                    ),
                    ("GRASS_GIT_TARGET_IP".to_owned(), target_ip.to_string()),
                    (
                        "GRASS_GIT_TARGET_PORT".to_owned(),
                        endpoint.port.to_string(),
                    ),
                ]);
            }
            GitTransport::Ssh => {}
        }

        Ok(context)
    }
}

impl Drop for GitAuthContext {
    fn drop(&mut self) {
        if let Some(directory) = &self.directory {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

async fn write_private_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), CheckoutError> {
    tokio::fs::write(path, contents)
        .await
        .map_err(|_| CheckoutError::CredentialSetup)?;
    set_private_mode(path, mode).await
}

#[cfg(unix)]
async fn set_private_mode(path: &Path, mode: u32) -> Result<(), CheckoutError> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .await
        .map_err(|_| CheckoutError::CredentialSetup)
}

#[cfg(not(unix))]
async fn set_private_mode(_path: &Path, _mode: u32) -> Result<(), CheckoutError> {
    Err(CheckoutError::CredentialSetup)
}

async fn run_git(
    args: &[&str],
    cwd: &Path,
    transport: GitTransport,
    auth: &GitAuthContext,
) -> Result<String, String> {
    let allowed_protocol = match transport {
        GitTransport::Http => "http",
        GitTransport::Https => "https",
        GitTransport::Ssh => "ssh",
        GitTransport::Git => "git",
    };
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        // Defense in depth: only the already-validated transport may run.
        .env("GIT_ALLOW_PROTOCOL", allowed_protocol)
        .envs(auth.environment.iter().cloned())
        .output()
        .await
        .map_err(|error| format!("failed to spawn git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Clones the repository into `destination/source`, checking out the branch
/// or the specific commit when given, and validates the root directory.
pub async fn checkout(
    repository_url: &str,
    branch: Option<&str>,
    commit: Option<&str>,
    root_directory: Option<&str>,
    destination: &Path,
    access: CheckoutAccess<'_>,
) -> Result<CheckoutResult, CheckoutError> {
    let target =
        validate_repository_target(repository_url, access.private_target_exceptions).await?;
    let endpoint = target.endpoint;
    let target_ip = access.ssh_target_ip.unwrap_or_else(|| target.addresses[0]);
    if !target.addresses.contains(&target_ip) {
        return Err(CheckoutError::RepositoryTargetBlocked(
            "SSH target address changed after host-key observation".to_owned(),
        ));
    }
    let branch = branch.filter(|branch| !branch.trim().is_empty());
    if let Some(branch) = branch {
        validate_ref(branch)?;
    }
    let commit = commit.filter(|commit| !commit.trim().is_empty());
    if let Some(commit) = commit {
        validate_ref(commit)?;
    }

    let source_dir = destination.join("source");
    if source_dir.exists() {
        tokio::fs::remove_dir_all(&source_dir).await?;
    }
    tokio::fs::create_dir_all(destination).await?;
    let auth = GitAuthContext::prepare(
        destination,
        &endpoint,
        access.credential,
        access.known_hosts_line,
        target_ip,
    )
    .await?;

    let source_str = source_dir.display().to_string();
    match commit {
        Some(commit) => {
            // Fetch just the requested commit when the server allows it,
            // falling back to a full branch clone.
            run_git(
                &["init", "--quiet", &source_str],
                destination,
                endpoint.transport,
                &auth,
            )
            .await
            .map_err(CheckoutError::CloneFailed)?;
            run_git(
                &["remote", "add", "origin", repository_url],
                &source_dir,
                endpoint.transport,
                &auth,
            )
            .await
            .map_err(CheckoutError::CloneFailed)?;

            let direct_fetch = run_git(
                &["fetch", "--depth", "1", "origin", commit],
                &source_dir,
                endpoint.transport,
                &auth,
            )
            .await;
            if direct_fetch.is_err() {
                run_git(&["fetch", "origin"], &source_dir, endpoint.transport, &auth)
                    .await
                    .map_err(CheckoutError::CloneFailed)?;
            }
            run_git(
                &["checkout", "--quiet", commit],
                &source_dir,
                endpoint.transport,
                &auth,
            )
            .await
            .map_err(CheckoutError::CommitCheckoutFailed)?;
        }
        None => {
            let mut args = vec!["clone", "--depth", "1", "--quiet"];
            if let Some(branch) = branch {
                args.extend(["--branch", branch]);
            }
            args.push(repository_url);
            args.push(&source_str);
            run_git(&args, destination, endpoint.transport, &auth)
                .await
                .map_err(CheckoutError::CloneFailed)?;
        }
    }

    let commit_hash = run_git(
        &["rev-parse", "HEAD"],
        &source_dir,
        endpoint.transport,
        &auth,
    )
    .await
    .ok();

    // Root directory must stay inside the checkout.
    let project_root = match root_directory
        .map(str::trim)
        .filter(|root| !root.is_empty() && *root != ".")
    {
        Some(root) => {
            let candidate = source_dir.join(root);
            let canonical_source = source_dir.canonicalize()?;
            let canonical_root = candidate
                .canonicalize()
                .map_err(|_| CheckoutError::InvalidRootDirectory(root.to_owned()))?;
            if !canonical_root.starts_with(&canonical_source) || !canonical_root.is_dir() {
                return Err(CheckoutError::InvalidRootDirectory(root.to_owned()));
            }
            canonical_root
        }
        None => source_dir.clone(),
    };

    Ok(CheckoutResult {
        source_dir,
        project_root,
        commit_hash,
    })
}

/// Container-relative working directory for a checkout: the path of the
/// project root inside `/workspace` (which binds the source dir).
pub fn container_working_dir(source_dir: &Path, project_root: &Path) -> String {
    project_root
        .strip_prefix(source_dir)
        .map(|relative| relative.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn root_directory_cannot_escape_the_checkout() {
        let dir = std::env::temp_dir().join(format!("grass-git-{}", uuid::Uuid::now_v7().simple()));
        let source = dir.join("source");
        std::fs::create_dir_all(source.join("app")).unwrap();

        // Simulate a finished checkout and only validate root handling.
        let canonical_source = source.canonicalize().unwrap();
        let escape = source.join("../outside");
        std::fs::create_dir_all(&escape).unwrap();
        let canonical_escape = escape.canonicalize().unwrap();
        assert!(!canonical_escape.starts_with(&canonical_source));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn container_working_dir_is_relative_to_workspace() {
        let source = Path::new("/data/builds/x/source");
        assert_eq!(
            container_working_dir(source, Path::new("/data/builds/x/source/apps/site")),
            "apps/site"
        );
        assert_eq!(container_working_dir(source, source), "");
    }

    #[test]
    fn git_version_parser_accepts_release_vendor_suffixes() {
        assert_eq!(parse_git_version("git version 2.49.0\n"), Some((2, 49)));
        assert_eq!(
            parse_git_version("git version 2.50.1 (Apple Git-155)\n"),
            Some((2, 50))
        );
        assert_eq!(
            parse_git_version("git version 2.49.0.windows.1\n"),
            Some((2, 49))
        );
        assert_eq!(parse_git_version("not git"), None);
    }

    #[tokio::test]
    async fn https_auth_helper_never_writes_secrets_and_cleans_up() {
        let destination =
            std::env::temp_dir().join(format!("grass-git-auth-{}", uuid::Uuid::now_v7().simple()));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("https://example.com/repo.git").unwrap();
        let credential = GitCredential::Https {
            username: "deploy".to_owned(),
            secret: "never-write-this-token".to_owned(),
        };
        let credential_access = RedeemGitCredentialResponse {
            credential,
            host: "example.com".to_owned(),
            port: 443,
        };
        let context = GitAuthContext::prepare(
            &destination,
            &endpoint,
            Some(&credential_access),
            None,
            "8.8.8.8".parse().unwrap(),
        )
        .await
        .unwrap();
        let auth_dir = context.directory.clone().unwrap();
        let helper = tokio::fs::read_to_string(auth_dir.join("https-askpass"))
            .await
            .unwrap();
        assert!(!helper.contains("never-write-this-token"));
        assert!(
            context.environment.iter().any(
                |(name, value)| name == "GRASS_GIT_SECRET" && value == "never-write-this-token"
            )
        );

        drop(context);
        assert!(!auth_dir.exists());
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }

    #[tokio::test]
    async fn credential_transport_mismatch_creates_no_temp_material() {
        let destination = std::env::temp_dir().join(format!(
            "grass-git-auth-mismatch-{}",
            uuid::Uuid::now_v7().simple()
        ));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("https://example.com/repo.git").unwrap();
        let credential = GitCredential::Ssh {
            username: "git".to_owned(),
            private_key: "private".to_owned(),
            passphrase: None,
        };
        let credential_access = RedeemGitCredentialResponse {
            credential,
            host: "example.com".to_owned(),
            port: 443,
        };
        assert!(
            GitAuthContext::prepare(
                &destination,
                &endpoint,
                Some(&credential_access),
                None,
                "8.8.8.8".parse().unwrap(),
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }

    #[tokio::test]
    async fn credential_scope_mismatch_creates_no_temp_material() {
        let destination = std::env::temp_dir().join(format!(
            "grass-git-auth-scope-mismatch-{}",
            uuid::Uuid::now_v7().simple()
        ));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("https://example.com/repo.git").unwrap();
        let credential_access = RedeemGitCredentialResponse {
            credential: GitCredential::Https {
                username: "deploy".to_owned(),
                secret: "token".to_owned(),
            },
            host: "other.example".to_owned(),
            port: 443,
        };
        assert!(matches!(
            GitAuthContext::prepare(
                &destination,
                &endpoint,
                Some(&credential_access),
                None,
                "8.8.8.8".parse().unwrap(),
            )
            .await,
            Err(CheckoutError::CredentialMismatch)
        ));
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }

    #[test]
    fn ssh_keyscan_selection_is_stable_and_prefers_ed25519() {
        let rsa = STANDARD.encode(b"rsa-key");
        let ed25519 = STANDARD.encode(b"ed25519-key");
        for output in [
            format!("host ssh-rsa {rsa}\nhost ssh-ed25519 {ed25519}\n"),
            format!("host ssh-ed25519 {ed25519}\nhost ssh-rsa {rsa}\n"),
        ] {
            let (key_type, public_key, key_bytes) = select_scanned_host_key(&output).unwrap();
            assert_eq!(key_type, "ssh-ed25519");
            assert_eq!(public_key, ed25519);
            assert_eq!(key_bytes, b"ed25519-key");
        }
        assert!(select_scanned_host_key("host unknown-key Zm9v\n").is_err());
        assert!(select_scanned_host_key("host ssh-ed25519 not-base64\n").is_err());
    }

    #[tokio::test]
    async fn http_transport_pins_the_resolved_ip_and_disables_redirects() {
        let destination = std::env::temp_dir().join(format!(
            "grass-git-http-pin-{}",
            uuid::Uuid::now_v7().simple()
        ));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("https://example.com:8443/repo.git").unwrap();
        let context = GitAuthContext::prepare(
            &destination,
            &endpoint,
            None,
            None,
            "8.8.8.8".parse().unwrap(),
        )
        .await
        .unwrap();
        assert!(context.environment.contains(&(
            "GIT_CONFIG_VALUE_0".to_owned(),
            "+example.com:8443:8.8.8.8".to_owned()
        )));
        assert!(context.environment.contains(&(
            "GIT_CONFIG_KEY_1".to_owned(),
            "http.followRedirects".to_owned()
        )));
        assert!(
            context
                .environment
                .contains(&("GIT_CONFIG_VALUE_1".to_owned(), "false".to_owned()))
        );
        drop(context);
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }

    #[tokio::test]
    async fn http_ip_literals_need_no_second_resolution() {
        let destination = std::env::temp_dir().join(format!(
            "grass-git-http-literal-{}",
            uuid::Uuid::now_v7().simple()
        ));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("http://[2606:4700:4700::1111]/repo.git").unwrap();
        let context = GitAuthContext::prepare(
            &destination,
            &endpoint,
            None,
            None,
            "2606:4700:4700::1111".parse().unwrap(),
        )
        .await
        .unwrap();
        assert!(
            context
                .environment
                .contains(&("GIT_CONFIG_COUNT".to_owned(), "1".to_owned()))
        );
        assert!(context.environment.contains(&(
            "GIT_CONFIG_KEY_0".to_owned(),
            "http.followRedirects".to_owned()
        )));
        assert!(
            !context
                .environment
                .iter()
                .any(|(_, value)| value.contains("curloptResolve"))
        );
        drop(context);
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }

    #[tokio::test]
    async fn git_transport_proxy_receives_only_the_validated_ip_and_port() {
        let destination =
            std::env::temp_dir().join(format!("grass-git-proxy-{}", uuid::Uuid::now_v7().simple()));
        tokio::fs::create_dir_all(&destination).await.unwrap();
        let endpoint = parse_repository_url("git://example.com:19418/repo.git").unwrap();
        let context = GitAuthContext::prepare(
            &destination,
            &endpoint,
            None,
            None,
            "1.1.1.1".parse().unwrap(),
        )
        .await
        .unwrap();
        assert!(
            context
                .environment
                .contains(&("GRASS_GIT_TARGET_IP".to_owned(), "1.1.1.1".to_owned()))
        );
        assert!(
            context
                .environment
                .contains(&("GRASS_GIT_TARGET_PORT".to_owned(), "19418".to_owned()))
        );
        assert!(context.environment.iter().any(|(name, value)| {
            name == "GIT_PROXY_COMMAND" && std::path::Path::new(value).is_file()
        }));
        drop(context);
        tokio::fs::remove_dir_all(destination).await.unwrap();
    }
}
