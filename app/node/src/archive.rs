use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zip::{CompressionMethod, ZipWriter, write::FileOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedBundle {
    pub path: PathBuf,
    pub file_name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub enum ArchiveError {
    Validation(String),
    Io(std::io::Error),
    Zip(zip::result::ZipError),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Validation(message) => f.write_str(message),
            Self::Io(error) => write!(f, "{error}"),
            Self::Zip(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(_) => None,
            Self::Io(error) => Some(error),
            Self::Zip(error) => Some(error),
        }
    }
}

pub fn archive_output_directory(output_dir: &Path) -> Result<ArchivedBundle, ArchiveError> {
    archive_output_directory_with_id(output_dir, Uuid::new_v4())
}

pub fn archive_output_directory_with_id(
    output_dir: &Path,
    deployment_id: Uuid,
) -> Result<ArchivedBundle, ArchiveError> {
    if !output_dir.is_dir() {
        return Err(ArchiveError::Validation(format!(
            "output directory does not exist: {}",
            output_dir.display()
        )));
    }

    if !output_dir.join("index.html").is_file() {
        return Err(ArchiveError::Validation(
            "output directory must contain index.html".to_owned(),
        ));
    }

    let file_name = format!("{deployment_id}.zip");
    let path = output_dir.parent().unwrap_or(output_dir).join(&file_name);
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut stack = vec![output_dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(ArchiveError::Io)? {
            let entry = entry.map_err(ArchiveError::Io)?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();

    for path in files {
        let relative_path = path
            .strip_prefix(output_dir)
            .map_err(|error| ArchiveError::Validation(error.to_string()))?;
        let entry_name = relative_path.to_string_lossy().replace('\\', "/");
        writer
            .start_file(entry_name, options)
            .map_err(ArchiveError::Zip)?;
        let mut file = std::fs::File::open(&path).map_err(ArchiveError::Io)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(ArchiveError::Io)?;
        writer.write_all(&buffer).map_err(ArchiveError::Io)?;
    }

    let bytes = writer.finish().map_err(ArchiveError::Zip)?.into_inner();
    std::fs::write(&path, &bytes).map_err(ArchiveError::Io)?;

    Ok(ArchivedBundle {
        path,
        file_name,
        bytes,
    })
}
