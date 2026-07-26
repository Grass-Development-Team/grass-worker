//! Safe zip packing and unpacking for Grass Output artifacts.
//!
//! Packing walks a directory deterministically and only includes regular
//! files. Unpacking validates every entry name so hostile archives cannot
//! write outside the destination directory.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Debug)]
pub struct PackedArchive {
    pub size_bytes: u64,
    pub checksum_sha256: String,
    pub file_count: usize,
}

/// Packs `source_dir` into a zip at `destination`. Entry names are relative
/// paths with forward slashes; symlinks and other non-regular files are
/// skipped so archives never capture host files through links.
pub fn pack_dir(source_dir: &Path, destination: &Path) -> anyhow::Result<PackedArchive> {
    if !source_dir.is_dir() {
        anyhow::bail!("archive source {} is not a directory", source_dir.display());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(destination)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut entries: Vec<PathBuf> = walkdir::WalkDir::new(source_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect();
    entries.sort();

    let mut file_count = 0;
    let mut buffer = Vec::new();
    for path in entries {
        let relative = path
            .strip_prefix(source_dir)
            .map_err(|_| anyhow::anyhow!("walked outside the archive source"))?;
        let name = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        writer.start_file(&name, options)?;
        buffer.clear();
        File::open(&path)?.read_to_end(&mut buffer)?;
        writer.write_all(&buffer)?;
        file_count += 1;
    }

    writer.finish()?;

    let bytes = std::fs::read(destination)?;
    Ok(PackedArchive {
        size_bytes: bytes.len() as u64,
        checksum_sha256: hex::encode(Sha256::digest(&bytes)),
        file_count,
    })
}

/// Validates a zip entry name and returns the safe relative path it may be
/// extracted to. Rejects absolute paths, drive prefixes, `..` segments, and
/// empty names.
pub fn sanitize_entry_name(name: &str) -> anyhow::Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    let path = Path::new(&normalized);
    let mut safe = PathBuf::new();
    let mut segments = 0;

    for component in path.components() {
        match component {
            Component::Normal(segment) => {
                safe.push(segment);
                segments += 1;
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("unsafe archive entry name: {name}");
            }
        }
    }

    if segments == 0 {
        anyhow::bail!("empty archive entry name");
    }
    Ok(safe)
}

/// Unpacks a zip archive into `destination`, refusing entries that would
/// escape it. Returns the number of extracted files.
pub fn unpack_zip(archive_path: &Path, destination: &Path) -> anyhow::Result<usize> {
    let file = File::open(archive_path)?;
    unpack_zip_reader(file, destination)
}

/// Unpacks zip bytes into `destination` with the same entry validation.
pub fn unpack_zip_bytes(bytes: &[u8], destination: &Path) -> anyhow::Result<usize> {
    unpack_zip_reader(std::io::Cursor::new(bytes), destination)
}

/// Upper bounds that keep a hostile archive from exhausting disk. A single
/// build artifact is expected to be far below these; they exist so a
/// decompression bomb fails loudly instead of filling the host.
const MAX_ENTRIES: usize = 50_000;
const MAX_TOTAL_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRY_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn unpack_zip_reader<R: Read + std::io::Seek>(
    reader: R,
    destination: &Path,
) -> anyhow::Result<usize> {
    let mut archive = ZipArchive::new(reader)?;
    if archive.len() > MAX_ENTRIES {
        anyhow::bail!(
            "archive has {} entries, exceeding the {} entry limit",
            archive.len(),
            MAX_ENTRIES
        );
    }
    std::fs::create_dir_all(destination)?;

    let mut extracted = 0;
    let mut total_written: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_owned();

        if entry.is_dir() {
            let safe = sanitize_entry_name(name.trim_end_matches('/'))?;
            std::fs::create_dir_all(destination.join(safe))?;
            continue;
        }

        let safe = sanitize_entry_name(&name)?;
        let target = destination.join(&safe);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        // Bound each entry independently, then re-check the running total so
        // neither one huge entry nor many small entries can overrun the caps.
        let remaining_total = MAX_TOTAL_UNPACKED_BYTES.saturating_sub(total_written);
        let entry_cap = MAX_ENTRY_UNPACKED_BYTES.min(remaining_total);
        let written = std::io::copy(&mut entry.by_ref().take(entry_cap + 1), &mut output)?;
        if written > entry_cap {
            anyhow::bail!(
                "archive entry {name} exceeds the unpack size limit; refusing decompression bomb"
            );
        }
        total_written += written;
        extracted += 1;
    }

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grass-archive-{label}-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn packs_and_unpacks_directories() {
        let source = temp_dir("source");
        std::fs::create_dir_all(source.join("static/assets")).unwrap();
        std::fs::write(source.join("output.toml"), "version = 1\n").unwrap();
        std::fs::write(source.join("static/index.html"), "<html></html>").unwrap();
        std::fs::write(source.join("static/assets/app.js"), "console.log(1)").unwrap();

        let archive_dir = temp_dir("archive");
        let archive_path = archive_dir.join("grass-output.zip");
        let packed = pack_dir(&source, &archive_path).unwrap();
        assert_eq!(packed.file_count, 3);
        assert!(packed.size_bytes > 0);
        assert_eq!(packed.checksum_sha256.len(), 64);

        let dest = temp_dir("dest");
        let extracted = unpack_zip(&archive_path, &dest).unwrap();
        assert_eq!(extracted, 3);
        assert_eq!(
            std::fs::read_to_string(dest.join("static/index.html")).unwrap(),
            "<html></html>"
        );

        for dir in [source, archive_dir, dest] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn entry_names_cannot_escape_the_destination() {
        assert!(sanitize_entry_name("static/index.html").is_ok());
        assert!(sanitize_entry_name("./static/app.js").is_ok());
        assert!(sanitize_entry_name("../evil.sh").is_err());
        assert!(sanitize_entry_name("static/../../evil.sh").is_err());
        assert!(sanitize_entry_name("/etc/passwd").is_err());
        assert!(sanitize_entry_name("").is_err());
        assert!(sanitize_entry_name("..").is_err());
    }

    #[test]
    fn hostile_archives_are_rejected_during_unpack() {
        // Build a zip whose entry tries to climb out of the destination.
        let dir = temp_dir("hostile");
        let archive_path = dir.join("evil.zip");
        {
            let file = File::create(&archive_path).unwrap();
            let mut writer = ZipWriter::new(file);
            writer
                .start_file("../outside.txt", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"escaped").unwrap();
            writer.finish().unwrap();
        }

        let dest = dir.join("dest");
        let error = unpack_zip(&archive_path, &dest).unwrap_err();
        assert!(error.to_string().contains("unsafe archive entry name"));
        assert!(!dir.join("outside.txt").exists());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn symlinks_are_not_packed() {
        let source = temp_dir("symlink");
        std::fs::write(source.join("real.txt"), "data").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", source.join("link.txt")).unwrap();

        let archive_dir = temp_dir("symlink-archive");
        let packed = pack_dir(&source, &archive_dir.join("out.zip")).unwrap();
        assert_eq!(packed.file_count, 1);

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(archive_dir).unwrap();
    }
}
