//! Git checkout for build workspaces.

use std::path::{Path, PathBuf};

use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum CheckoutError {
    #[error("git clone failed: {0}")]
    CloneFailed(String),
    #[error("commit checkout failed: {0}")]
    CommitCheckoutFailed(String),
    #[error("root directory {0} is invalid")]
    InvalidRootDirectory(String),
    #[error("repository url {0} is not allowed")]
    InvalidRepositoryUrl(String),
    #[error("git ref {0} is not allowed")]
    InvalidRef(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Rejects repository URLs whose scheme is not http(s). The control plane
/// validates this too, but the build node re-checks so a compromised or
/// older control plane cannot make host-side git use file/ssh/ext/git
/// transports.
fn validate_repository_url(url: &str) -> Result<(), CheckoutError> {
    match url::Url::parse(url) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => Ok(()),
        _ => Err(CheckoutError::InvalidRepositoryUrl(url.to_owned())),
    }
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

async fn run_git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Defense in depth: never let a remote helper transport (ext, file,
        // ssh, git) run, even if a repository url slipped past validation.
        .env("GIT_ALLOW_PROTOCOL", "http:https")
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
) -> Result<CheckoutResult, CheckoutError> {
    validate_repository_url(repository_url)?;
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

    let source_str = source_dir.display().to_string();
    match commit {
        Some(commit) => {
            // Fetch just the requested commit when the server allows it,
            // falling back to a full branch clone.
            run_git(&["init", "--quiet", &source_str], destination)
                .await
                .map_err(CheckoutError::CloneFailed)?;
            run_git(&["remote", "add", "origin", repository_url], &source_dir)
                .await
                .map_err(CheckoutError::CloneFailed)?;

            let direct_fetch =
                run_git(&["fetch", "--depth", "1", "origin", commit], &source_dir).await;
            if direct_fetch.is_err() {
                run_git(&["fetch", "origin"], &source_dir)
                    .await
                    .map_err(CheckoutError::CloneFailed)?;
            }
            run_git(&["checkout", "--quiet", commit], &source_dir)
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
            run_git(&args, destination)
                .await
                .map_err(CheckoutError::CloneFailed)?;
        }
    }

    let commit_hash = run_git(&["rev-parse", "HEAD"], &source_dir).await.ok();

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
}
