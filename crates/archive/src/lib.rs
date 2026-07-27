//! Safe zip packing and unpacking for Grass Output artifacts.
//!
//! Packing walks a directory deterministically and only includes regular
//! files. Unpacking validates every entry name so hostile archives cannot
//! write outside the destination directory.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[derive(Debug)]
pub struct PackedArchive {
    pub size_bytes: u64,
    pub unpacked_size_bytes: u64,
    pub checksum_sha256: String,
    pub file_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnpackedArchive {
    pub file_count: usize,
    pub unpacked_size_bytes: u64,
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
    let mut unpacked_size_bytes = 0_u64;
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
        let mut source = File::open(&path)?;
        unpacked_size_bytes = unpacked_size_bytes
            .checked_add(std::io::copy(&mut source, &mut writer)?)
            .ok_or_else(|| anyhow::anyhow!("archive unpacked size overflow"))?;
        file_count += 1;
    }

    writer.finish()?;

    let size_bytes = std::fs::metadata(destination)?.len();
    let mut archive = File::open(destination)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = archive.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(PackedArchive {
        size_bytes,
        unpacked_size_bytes,
        checksum_sha256: hex::encode(hasher.finalize()),
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
/// escape it. Returns the extracted file count and total unpacked bytes.
pub fn unpack_zip(archive_path: &Path, destination: &Path) -> anyhow::Result<UnpackedArchive> {
    let file = File::open(archive_path)?;
    unpack_zip_reader(file, destination)
}

/// Unpacks zip bytes into `destination` with the same entry validation.
pub fn unpack_zip_bytes(bytes: &[u8], destination: &Path) -> anyhow::Result<UnpackedArchive> {
    unpack_zip_reader(std::io::Cursor::new(bytes), destination)
}

/// Upper bounds that keep a hostile archive from exhausting disk. A single
/// build artifact is expected to be far below these; they exist so a
/// decompression bomb fails loudly instead of filling the host.
const MAX_ENTRIES: usize = 50_000;
const MAX_TOTAL_UNPACKED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRY_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy)]
struct UnpackLimits {
    entries: usize,
    entry_bytes: u64,
    total_bytes: u64,
}

const DEFAULT_UNPACK_LIMITS: UnpackLimits = UnpackLimits {
    entries: MAX_ENTRIES,
    entry_bytes: MAX_ENTRY_UNPACKED_BYTES,
    total_bytes: MAX_TOTAL_UNPACKED_BYTES,
};

fn unpack_zip_reader<R: Read + std::io::Seek>(
    reader: R,
    destination: &Path,
) -> anyhow::Result<UnpackedArchive> {
    unpack_zip_reader_with_limits(reader, destination, DEFAULT_UNPACK_LIMITS)
}

fn unpack_zip_reader_with_limits<R: Read + std::io::Seek>(
    reader: R,
    destination: &Path,
    limits: UnpackLimits,
) -> anyhow::Result<UnpackedArchive> {
    let mut archive = ZipArchive::new(reader)?;
    if archive.len() > limits.entries {
        anyhow::bail!(
            "archive has {} entries, exceeding the {} entry limit",
            archive.len(),
            limits.entries
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

        if entry.size() > limits.entry_bytes {
            anyhow::bail!("archive entry {name} exceeds the per-entry unpack size limit");
        }
        if entry.size() > limits.total_bytes.saturating_sub(total_written) {
            anyhow::bail!("archive exceeds the total unpack size limit");
        }

        let safe = sanitize_entry_name(&name)?;
        let target = destination.join(&safe);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&target)?;
        // Bound each entry independently, then re-check the running total so
        // neither one huge entry nor many small entries can overrun the caps.
        let remaining_total = limits.total_bytes.saturating_sub(total_written);
        let entry_cap = limits.entry_bytes.min(remaining_total);
        let written = std::io::copy(&mut entry.by_ref().take(entry_cap + 1), &mut output)?;
        if written > limits.entry_bytes {
            anyhow::bail!("archive entry {name} exceeds the per-entry unpack size limit");
        }
        if written > remaining_total {
            anyhow::bail!("archive exceeds the total unpack size limit");
        }
        total_written += written;
        extracted += 1;
    }

    Ok(UnpackedArchive {
        file_count: extracted,
        unpacked_size_bytes: total_written,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grass-archive-{label}-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut bytes);
            for (name, contents) in entries {
                writer
                    .start_file(*name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        bytes.into_inner()
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
        let unpacked = unpack_zip(&archive_path, &dest).unwrap();
        assert_eq!(unpacked.file_count, 3);
        assert_eq!(
            unpacked.unpacked_size_bytes,
            ("version = 1\n".len() + "<html></html>".len() + "console.log(1)".len()) as u64
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("static/index.html")).unwrap(),
            "<html></html>"
        );

        for dir in [source, archive_dir, dest] {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn packed_archive_reports_unpacked_bytes() {
        let source = temp_dir("unpacked-size");
        std::fs::write(source.join("a"), b"123").unwrap();
        std::fs::write(source.join("b"), b"4567").unwrap();
        let archive_dir = temp_dir("unpacked-size-archive");

        let packed = pack_dir(&source, &archive_dir.join("out.zip")).unwrap();

        assert_eq!(packed.unpacked_size_bytes, 7);
        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(archive_dir).unwrap();
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

    #[test]
    fn rejects_archives_over_the_entry_count_limit() {
        let bytes = zip_bytes(&[("one", b""), ("two", b""), ("three", b"")]);
        let destination = temp_dir("entry-count-limit");
        let error = unpack_zip_reader_with_limits(
            std::io::Cursor::new(bytes),
            &destination,
            UnpackLimits {
                entries: 2,
                entry_bytes: 100,
                total_bytes: 100,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("entry limit"));
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn rejects_archives_over_the_per_entry_byte_limit() {
        let bytes = zip_bytes(&[("large", b"12345")]);
        let destination = temp_dir("entry-byte-limit");
        let error = unpack_zip_reader_with_limits(
            std::io::Cursor::new(bytes),
            &destination,
            UnpackLimits {
                entries: 10,
                entry_bytes: 4,
                total_bytes: 100,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("per-entry"));
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn rejects_archives_over_the_total_byte_limit() {
        let bytes = zip_bytes(&[("one", b"1234"), ("two", b"5678")]);
        let destination = temp_dir("total-byte-limit");
        let error = unpack_zip_reader_with_limits(
            std::io::Cursor::new(bytes),
            &destination,
            UnpackLimits {
                entries: 10,
                entry_bytes: 10,
                total_bytes: 7,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("total unpack"));
        std::fs::remove_dir_all(destination).unwrap();
    }
}
