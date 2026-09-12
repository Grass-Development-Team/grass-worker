//! Container archive I/O has separate wire, logical-content and entry budgets.
//! Temporary files keep large transfers off the Node heap and are removed on drop.
use super::ContainerRuntimeError;
use futures_util::{Stream, StreamExt};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, sync::watch};

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_METADATA_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub entries: usize,
    pub entry_bytes: u64,
    pub content_bytes: u64,
    pub wire_bytes: u64,
}
pub(super) const LIMITS: Limits = Limits {
    entries: 50_000,
    entry_bytes: 2 * 1024 * 1024 * 1024,
    content_bytes: 4 * 1024 * 1024 * 1024,
    wire_bytes: 4 * 1024 * 1024 * 1024 + 64 * 1024 * 1024,
};

#[derive(Clone)]
pub(super) struct TransferControl {
    cancel: watch::Receiver<bool>,
    deadline: Instant,
}
impl TransferControl {
    pub fn new(cancel: watch::Receiver<bool>) -> Self {
        Self {
            cancel,
            deadline: Instant::now() + TRANSFER_TIMEOUT,
        }
    }
    fn check(&self) -> io::Result<()> {
        if *self.cancel.borrow() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "archive transfer canceled",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "archive transfer timed out",
            ));
        }
        Ok(())
    }
    pub async fn run<F: Future>(&self, future: F) -> Result<F::Output, ContainerRuntimeError> {
        if *self.cancel.borrow() {
            return Err(ContainerRuntimeError::Canceled);
        }
        let mut cancel = self.cancel.clone();
        tokio::select! {
            result = future => Ok(result),
            _ = tokio::time::sleep_until(self.deadline.into()) => Err(ContainerRuntimeError::Timeout(TRANSFER_TIMEOUT.as_secs())),
            _ = async {
                if cancel.wait_for(|value| *value).await.is_err() { std::future::pending::<()>().await; }
            } => Err(ContainerRuntimeError::Canceled),
        }
    }
}

fn error(error: impl std::fmt::Display) -> ContainerRuntimeError {
    ContainerRuntimeError::Runtime(format!("archive: {error}"))
}
fn exceeded() -> io::Error {
    io::Error::other("container archive exceeds its resource budget")
}

#[derive(Clone, Copy)]
pub(super) struct Budget {
    limits: Limits,
    entries: usize,
    content: u64,
    wire: u64,
    names: u64,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            entries: 0,
            content: 0,
            wire: 0,
            names: 0,
        }
    }
    fn entry(&mut self, bytes: u64) -> io::Result<()> {
        if self.entries >= self.limits.entries
            || bytes > self.limits.entry_bytes
            || bytes > self.limits.content_bytes.saturating_sub(self.content)
        {
            return Err(exceeded());
        }
        self.entries += 1;
        self.content += bytes;
        Ok(())
    }
    fn names(&mut self, bytes: u64) -> io::Result<()> {
        if bytes > (16 * 1024 * 1024u64).saturating_sub(self.names) {
            return Err(exceeded());
        }
        self.names += bytes;
        Ok(())
    }
    fn wire(&mut self, bytes: u64) -> io::Result<()> {
        if bytes > self.limits.wire_bytes.saturating_sub(self.wire) {
            return Err(exceeded());
        }
        self.wire += bytes;
        Ok(())
    }
}

struct CheckedIo<T> {
    inner: T,
    control: TransferControl,
    remaining: u64,
}
impl<T: Write> Write for CheckedIo<T> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.control.check()?;
        if bytes.len() as u64 > self.remaining {
            return Err(exceeded());
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.control.check()?;
        self.inner.flush()
    }
}
impl<T: Read> Read for CheckedIo<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.control.check()?;
        self.inner.read(bytes)
    }
}

pub(super) async fn pack(
    root: &'static str,
    directory: PathBuf,
    limits: Limits,
    control: TransferControl,
) -> Result<tokio::fs::File, ContainerRuntimeError> {
    // Await the worker: cancellation is checked on each chunk, so no writer survives cleanup.
    let file = tokio::task::spawn_blocking(move || -> io::Result<File> {
        let mut budget = Budget::new(limits);
        let writer = CheckedIo {
            inner: tempfile::tempfile()?,
            control: control.clone(),
            remaining: limits.wire_bytes,
        };
        let mut builder = tar::Builder::new(writer);
        builder.follow_symlinks(false);
        builder.sparse(false);
        for entry in walkdir::WalkDir::new(&directory)
            .follow_links(false)
            .max_open(16)
        {
            control.check()?;
            let entry = entry.map_err(io::Error::other)?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            if !(metadata.is_file() || metadata.is_dir() || metadata.is_symlink()) {
                return Err(io::Error::other("unsupported archive file type"));
            }
            budget.entry(if metadata.is_file() {
                metadata.len()
            } else {
                0
            })?;
            let relative = entry
                .path()
                .strip_prefix(&directory)
                .map_err(io::Error::other)?;
            let target = Path::new(root).join(relative);
            budget.names(target.as_os_str().as_encoded_bytes().len() as u64)?;
            if metadata.is_file() {
                // Explicit header and checked reader prevent growth or sparse expansion bypasses.
                let mut header = tar::Header::new_gnu();
                header.set_metadata(&metadata);
                header.set_size(metadata.len());
                header.set_entry_type(tar::EntryType::Regular);
                let reader = CheckedIo {
                    inner: File::open(entry.path())?,
                    control: control.clone(),
                    remaining: 0,
                };
                builder.append_data(&mut header, target, reader.take(metadata.len()))?;
            } else {
                builder.append_path_with_name(entry.path(), target)?;
            }
        }
        let mut file = builder.into_inner()?.inner;
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    })
    .await
    .map_err(error)?
    .map_err(error)?;
    Ok(tokio::fs::File::from_std(file))
}

pub(super) async fn download<S, B>(
    mut stream: S,
    budget: &mut Budget,
    control: &TransferControl,
) -> Result<Option<File>, ContainerRuntimeError>
where
    S: Stream<Item = Result<B, bollard::errors::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    let mut file = tokio::fs::File::from_std(tempfile::tempfile().map_err(error)?);
    let mut received = false;
    while let Some(chunk) = control.run(stream.next()).await? {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) if !received => return Ok(None),
            Err(err) => return Err(error(err)),
        };
        budget.wire(chunk.as_ref().len() as u64).map_err(error)?;
        received |= !chunk.as_ref().is_empty();
        control
            .run(file.write_all(chunk.as_ref()))
            .await?
            .map_err(error)?;
    }
    if !received {
        return Err(error("empty export response"));
    }
    control.run(file.flush()).await?.map_err(error)?;
    let mut file = file.into_std().await;
    file.seek(SeekFrom::Start(0)).map_err(error)?;
    Ok(Some(file))
}

// Validate raw headers before the tar library can allocate GNU/PAX metadata.
// PAX size overrides must agree with the raw header, making both passes see the same layout.
fn preflight(file: &mut File, budget: &mut Budget, control: &TransferControl) -> io::Result<()> {
    let reader = CheckedIo {
        inner: &mut *file,
        control: control.clone(),
        remaining: 0,
    };
    let mut archive = tar::Archive::new(reader);
    let mut pax_size = None;
    for entry in archive.entries()?.raw(true) {
        control.check()?;
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        let bytes = entry.size();
        let extension =
            kind.is_gnu_longname() || kind.is_gnu_longlink() || kind.is_pax_local_extensions();
        if extension {
            if bytes > MAX_METADATA_BYTES {
                return Err(exceeded());
            }
            budget.entry(0)?;
            budget.names(bytes)?;
            if kind.is_pax_local_extensions() {
                if let Some(fields) = entry.pax_extensions()? {
                    for field in fields {
                        let field = field?;
                        let key = field.key().map_err(io::Error::other)?;
                        if key.starts_with("GNU.sparse") {
                            return Err(io::Error::other("sparse export metadata is unsupported"));
                        }
                        if key == "size" {
                            pax_size = Some(
                                field
                                    .value()
                                    .map_err(io::Error::other)?
                                    .parse::<u64>()
                                    .map_err(io::Error::other)?,
                            );
                        }
                    }
                }
            }
            continue;
        }
        if !(kind.is_file() || kind.is_dir() || kind.is_symlink() || kind.is_hard_link()) {
            return Err(io::Error::other("unsupported export entry type"));
        }
        if pax_size.take().is_some_and(|size| size != bytes) {
            return Err(io::Error::other("inconsistent PAX entry size"));
        }
        budget.names(
            entry.path_bytes().len() as u64
                + entry.link_name_bytes().map_or(0, |name| name.len()) as u64,
        )?;
        budget.entry(bytes)?;
    }
    if pax_size.is_some() {
        return Err(io::Error::other("orphaned PAX metadata"));
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

pub(super) async fn unpack(
    mut file: File,
    destination: PathBuf,
    root: String,
    mut budget: Budget,
    control: TransferControl,
) -> Result<Budget, ContainerRuntimeError> {
    tokio::task::spawn_blocking(move || -> io::Result<Budget> {
        let content_before = budget.content;
        preflight(&mut file, &mut budget, &control)?;
        let mut archive = tar::Archive::new(CheckedIo {
            inner: file,
            control: control.clone(),
            remaining: 0,
        });
        for entry in archive.entries()? {
            control.check()?;
            let mut entry = entry?;
            let path = entry.path()?;
            if !path.starts_with(&root) {
                return Err(io::Error::other("export entry is outside requested output"));
            }
            if !entry.unpack_in(&destination)? {
                return Err(io::Error::other("export entry escapes destination"));
            }
        }
        if std::fs::symlink_metadata(destination.join(&root))?
            .file_type()
            .is_symlink()
        {
            return Err(io::Error::other("export root must not be a symbolic link"));
        }
        // Hard links are cheap in tar, but output adaptation copies their contents.
        // Count each final file's logical length before making the export available.
        budget.content = content_before;
        for entry in walkdir::WalkDir::new(&destination).follow_links(false) {
            control.check()?;
            let entry = entry.map_err(io::Error::other)?;
            if entry.file_type().is_file() {
                let bytes = entry.metadata().map_err(io::Error::other)?.len();
                if bytes > budget.limits.entry_bytes
                    || bytes > budget.limits.content_bytes.saturating_sub(budget.content)
                {
                    return Err(exceeded());
                }
                budget.content += bytes;
            }
        }
        Ok(budget)
    })
    .await
    .map_err(error)?
    .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn control() -> TransferControl {
        let (_, rx) = watch::channel(false);
        TransferControl::new(rx)
    }
    fn limits() -> Limits {
        Limits {
            entries: 8,
            entry_bytes: 32,
            content_bytes: 48,
            wire_bytes: 8192,
        }
    }
    fn tar(entries: &[(&str, &[u8])]) -> File {
        let mut builder = tar::Builder::new(tempfile::tempfile().unwrap());
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        let mut file = builder.into_inner().unwrap();
        file.rewind().unwrap();
        file
    }
    #[tokio::test]
    async fn rejects_large_entries_and_combined_exports_before_unpacking() {
        let dest = tempfile::tempdir().unwrap();
        assert!(
            unpack(
                tar(&[("dist/large", &[0; 33])]),
                dest.path().into(),
                "dist".into(),
                Budget::new(limits()),
                control()
            )
            .await
            .is_err()
        );
        assert!(!dest.path().join("dist/large").exists());
        let first = unpack(
            tar(&[("dist/a", &[0; 30])]),
            dest.path().into(),
            "dist".into(),
            Budget::new(limits()),
            control(),
        )
        .await
        .unwrap();
        assert!(
            unpack(
                tar(&[("dist/b", &[0; 30])]),
                dest.path().into(),
                "dist".into(),
                first,
                control()
            )
            .await
            .is_err()
        );
        assert!(!dest.path().join("dist/b").exists());
    }
    #[tokio::test]
    async fn bounds_entry_count_and_cancellation() {
        let dest = tempfile::tempdir().unwrap();
        let mut limit = limits();
        limit.entries = 1;
        assert!(
            unpack(
                tar(&[("dist/a", b"a"), ("dist/b", b"b")]),
                dest.path().into(),
                "dist".into(),
                Budget::new(limit),
                control()
            )
            .await
            .is_err()
        );
        let (_, rx) = watch::channel(true);
        assert!(
            unpack(
                tar(&[("dist/a", b"a")]),
                dest.path().into(),
                "dist".into(),
                Budget::new(limits()),
                TransferControl::new(rx)
            )
            .await
            .is_err()
        );
        assert!(!dest.path().join("dist").exists());
    }
    #[tokio::test]
    async fn wire_budget_and_pending_transfer_timeout_are_enforced() {
        let mut limit = limits();
        limit.wire_bytes = 3;
        let stream = futures_util::stream::iter([Ok(vec![0u8; 2]), Ok(vec![0u8; 2])]);
        assert!(
            download(stream, &mut Budget::new(limit), &control())
                .await
                .is_err()
        );
        let mut expired = control();
        expired.deadline = Instant::now();
        assert!(expired.run(std::future::pending::<()>()).await.is_err());
    }
    #[tokio::test]
    async fn upload_limit_is_checked_before_reading_a_large_file() {
        let source = tempfile::tempdir().unwrap();
        File::create(source.path().join("large"))
            .unwrap()
            .set_len(33)
            .unwrap();
        assert!(
            pack("workspace", source.path().into(), limits(), control())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn hard_link_expansion_counts_against_the_content_budget() {
        let mut builder = tar::Builder::new(tempfile::tempfile().unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_size(30);
        header.set_mode(0o644);
        builder
            .append_data(&mut header, "dist/original", &[0u8; 30][..])
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Link);
        builder
            .append_link(&mut header, "dist/link", "dist/original")
            .unwrap();
        let mut file = builder.into_inner().unwrap();
        file.rewind().unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(
            unpack(
                file,
                dest.path().into(),
                "dist".into(),
                Budget::new(limits()),
                control()
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn rejects_oversized_extension_metadata_before_decoding_it() {
        let mut builder = tar::Builder::new(tempfile::tempfile().unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_size(MAX_METADATA_BYTES + 1);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::GNULongName);
        builder
            .append_data(
                &mut header,
                "././@LongLink",
                std::io::repeat(0).take(MAX_METADATA_BYTES + 1),
            )
            .unwrap();
        let mut file = builder.into_inner().unwrap();
        file.rewind().unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(
            unpack(
                file,
                dest.path().into(),
                "dist".into(),
                Budget::new(LIMITS),
                control()
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn long_paths_round_trip_through_bounded_upload_and_export() {
        let source = tempfile::tempdir().unwrap();
        let name = "a".repeat(150);
        std::fs::write(source.path().join(&name), b"normal").unwrap();
        let file = pack("dist", source.path().into(), LIMITS, control())
            .await
            .unwrap()
            .into_std()
            .await;
        let dest = tempfile::tempdir().unwrap();
        unpack(
            file,
            dest.path().into(),
            "dist".into(),
            Budget::new(LIMITS),
            control(),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(dest.path().join("dist").join(name)).unwrap(),
            b"normal"
        );
    }
    #[tokio::test]
    async fn exported_root_links_are_rejected_before_output_adaptation() {
        let mut builder = tar::Builder::new(tempfile::tempfile().unwrap());
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o777);
        header.set_entry_type(tar::EntryType::Symlink);
        builder
            .append_link(&mut header, "dist", "/outside")
            .unwrap();
        let mut file = builder.into_inner().unwrap();
        file.rewind().unwrap();
        let dest = tempfile::tempdir().unwrap();
        assert!(
            unpack(
                file,
                dest.path().into(),
                "dist".into(),
                Budget::new(LIMITS),
                control()
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn a_stalled_download_observes_cancellation() {
        let (sender, receiver) = watch::channel(false);
        let control = TransferControl::new(receiver);
        let cancel = async move {
            tokio::task::yield_now().await;
            sender.send(true).unwrap();
        };
        let receive = async {
            let stream = futures_util::stream::pending::<Result<Vec<u8>, bollard::errors::Error>>();
            download(stream, &mut Budget::new(LIMITS), &control).await
        };
        let (_, result) = tokio::join!(cancel, receive);
        assert!(matches!(result, Err(ContainerRuntimeError::Canceled)));
    }
}
