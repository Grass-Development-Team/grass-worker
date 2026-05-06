use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStaticSite {
    pub root_dir: PathBuf,
    pub checksum_sha256: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSiteStorageError {
    InvalidArchive(&'static str),
    Io(String),
}

impl std::fmt::Display for StaticSiteStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArchive(message) => f.write_str(message),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StaticSiteStorageError {}

fn artifact_root() -> PathBuf {
    test_artifact_root()
        .or_else(|| std::env::var_os("GRASS_WORKER_ARTIFACT_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| std::env::temp_dir().join("grass-worker-artifacts"))
}

#[cfg(test)]
static TEST_ARTIFACT_ROOT: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
fn test_artifact_root() -> Option<PathBuf> {
    TEST_ARTIFACT_ROOT
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap()
        .clone()
}

#[cfg(not(test))]
fn test_artifact_root() -> Option<PathBuf> {
    None
}

#[cfg(test)]
pub fn set_test_artifact_root(path: PathBuf) {
    *TEST_ARTIFACT_ROOT
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap() = Some(path);
}

fn normalize_archive_path(path: &str) -> Result<PathBuf, StaticSiteStorageError> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(StaticSiteStorageError::InvalidArchive(
            "archive entry must be relative",
        ));
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err(StaticSiteStorageError::InvalidArchive(
                    "archive entry escapes target directory",
                ));
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(StaticSiteStorageError::InvalidArchive(
            "archive entry path is empty",
        ));
    }

    Ok(normalized)
}

pub fn store_uploaded_zip(
    project_id: Uuid,
    deployment_id: Uuid,
    file_name: Option<&str>,
    bytes: &[u8],
) -> Result<StoredStaticSite, StaticSiteStorageError> {
    let extension = file_name
        .and_then(|name| Path::new(name).extension())
        .and_then(OsStr::to_str)
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref() != Some("zip") {
        return Err(StaticSiteStorageError::InvalidArchive(
            "static site bundle must be a .zip archive",
        ));
    }

    let size_bytes = i64::try_from(bytes.len()).map_err(|_error| {
        StaticSiteStorageError::InvalidArchive("archive is too large to be stored")
    })?;
    let checksum_sha256 = hex::encode(Sha256::digest(bytes));

    let target_root = artifact_root()
        .join(project_id.to_string())
        .join(deployment_id.to_string())
        .join(&checksum_sha256);
    if target_root.exists() {
        std::fs::remove_dir_all(&target_root)
            .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;
    }
    std::fs::create_dir_all(&target_root)
        .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;

    let cursor = std::io::Cursor::new(bytes);
    extract_zip_into(cursor, &target_root)?;

    Ok(StoredStaticSite {
        root_dir: target_root,
        checksum_sha256,
        size_bytes,
    })
}

fn extract_zip_into<R: Read + Seek>(
    reader: R,
    target_root: &Path,
) -> Result<(), StaticSiteStorageError> {
    let mut archive = ZipArchive::new(reader).map_err(|_error| {
        StaticSiteStorageError::InvalidArchive("bundle is not a valid zip archive")
    })?;
    let mut extracted_any_file = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_error| {
            StaticSiteStorageError::InvalidArchive("bundle contains unreadable zip entries")
        })?;
        let relative_path = normalize_archive_path(entry.name())?;
        let output_path = target_root.join(relative_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&output_path)
                .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;
        }

        let mut output = File::create(&output_path)
            .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| StaticSiteStorageError::Io(error.to_string()))?;
        extracted_any_file = true;
    }

    if !extracted_any_file {
        return Err(StaticSiteStorageError::InvalidArchive(
            "bundle does not contain any files",
        ));
    }

    if !target_root.join("index.html").exists() {
        return Err(StaticSiteStorageError::InvalidArchive(
            "bundle must contain a root index.html file",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::{CompressionMethod, ZipWriter, write::FileOptions};

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);

        for (path, content) in entries {
            writer.start_file(*path, options).unwrap();
            writer.write_all(content).unwrap();
        }

        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn store_uploaded_zip_extracts_index_html() {
        let root = tempdir().unwrap();
        set_test_artifact_root(root.path().to_path_buf());

        let stored = store_uploaded_zip(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Some("site.zip"),
            &zip_bytes(&[("index.html", b"hello")]),
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(stored.root_dir.join("index.html")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn store_uploaded_zip_rejects_path_traversal() {
        let root = tempdir().unwrap();
        set_test_artifact_root(root.path().to_path_buf());

        let error = store_uploaded_zip(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Some("site.zip"),
            &zip_bytes(&[("../index.html", b"nope")]),
        )
        .unwrap_err();

        assert_eq!(
            error,
            StaticSiteStorageError::InvalidArchive("archive entry escapes target directory")
        );
    }
}
