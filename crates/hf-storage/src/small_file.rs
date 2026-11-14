//! Small engine-owned files in the state root: the lost-WAL acceptance record, the
//! verification fence and the verification due record, and the listed open used by the
//! commit watermark.
//!
//! Reads are `lstat`-first, bounded, and require the opened file to be the one listed.
//! Replacement writes a fresh temporary file and renames it over the target, so a reader
//! sees the old or the new content, never a torn mix.
use crate::{Error, Result};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

/// What a bounded read found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Content {
    Absent,
    /// A regular file longer than the bound.
    Oversized,
    Bytes(Vec<u8>),
}

/// The file's bytes, or `None` when it is absent. A non-regular or oversized file, or a
/// file that is not the one `lstat` listed (device/inode mismatch after open, including a
/// FIFO swapped in), is `Incompatible`. A symlink swapped in after `lstat` fails the
/// no-follow open with `ELOOP` and is returned as `Io`. Never creates or writes anything.
pub(crate) fn read(root: &Path, name: &str, bound: u64) -> Result<Option<Vec<u8>>> {
    match read_bounded(root, name, bound)? {
        Content::Absent => Ok(None),
        Content::Oversized => Err(Error::Incompatible),
        Content::Bytes(bytes) => Ok(Some(bytes)),
    }
}

/// As `read`, but a regular file over `bound` is `Oversized` rather than an error. A
/// non-regular file (directory, link, FIFO, device) or a regular file replaced after
/// `lstat` stays `Incompatible`; a symlink swapped in after `lstat` is `Io` (`ELOOP`).
pub(crate) fn read_bounded(root: &Path, name: &str, bound: u64) -> Result<Content> {
    let path = root.join(name);
    let listed = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Content::Absent),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_file() => return Err(Error::Incompatible),
        Ok(metadata) if metadata.len() > bound => return Ok(Content::Oversized),
        Ok(metadata) => metadata,
    };
    let file = open_no_follow(&path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file(&listed, &opened) {
        return Err(Error::Incompatible);
    }
    let mut bytes = Vec::new();
    file.take(bound + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > bound {
        return Ok(Content::Oversized);
    }
    Ok(Content::Bytes(bytes))
}

/// Opens for reading without following a final symlink (the open fails with `ELOOP`) and
/// without blocking on a FIFO swapped in after `lstat` (the `fstat` that follows then
/// refuses it). hf-storage has no
/// libc dependency, so the flag values are spelled out per platform: Darwin
/// `<sys/fcntl.h>` `O_NONBLOCK 0x0004`, `O_NOFOLLOW 0x0100`; Linux
/// `<asm-generic/fcntl.h>` `O_NONBLOCK 00004000`, `O_NOFOLLOW 00400000`, with arm and
/// aarch64 overriding `O_NOFOLLOW 0100000` (`arch/arm*/include/uapi/asm/fcntl.h`). Other
/// unix targets open without them: a link or FIFO swapped in between `lstat` and `open`
/// can be followed or block there (residual risk, as for the marker read).
#[cfg(unix)]
fn open_no_follow(path: &Path) -> std::io::Result<fs::File> {
    open_no_follow_with(path, false)
}

/// As `open_no_follow`, read-write when `write` is set. Never creates the file.
#[cfg(unix)]
fn open_no_follow_with(path: &Path, write: bool) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(write)
        .custom_flags(NO_FOLLOW_NONBLOCK)
        .open(path)
}

/// Opens `name` without creating it, read-write when `write` is set, and returns it with
/// its length, or `None` when it is absent. The same refusals as `read`: a non-regular
/// file, or one that is not the file `lstat` listed, is `Incompatible`; a symlink swapped
/// in after `lstat` is `Io` (`ELOOP`).
pub(crate) fn open_listed(root: &Path, name: &str, write: bool) -> Result<Option<(fs::File, u64)>> {
    let path = root.join(name);
    let listed = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.is_file() => return Err(Error::Incompatible),
        Ok(metadata) => metadata,
    };
    let file = open_no_follow_with(&path, write)?;
    let opened = file.metadata()?;
    if !opened.is_file() || !same_file(&listed, &opened) {
        return Err(Error::Incompatible);
    }
    Ok(Some((file, opened.len())))
}

#[cfg(target_os = "macos")]
const NO_FOLLOW_NONBLOCK: i32 = 0x0004 | 0x0100;
#[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
const NO_FOLLOW_NONBLOCK: i32 = 0o4000 | 0o100000;
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
const NO_FOLLOW_NONBLOCK: i32 = 0o4000 | 0o400000;
#[cfg(all(
    unix,
    not(target_os = "macos"),
    not(all(
        target_os = "linux",
        any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "x86_64",
            target_arch = "x86"
        )
    ))
))]
const NO_FOLLOW_NONBLOCK: i32 = 0;

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> std::io::Result<fs::File> {
    fs::File::open(path)
}

#[cfg(not(unix))]
fn open_no_follow_with(path: &Path, write: bool) -> std::io::Result<fs::File> {
    OpenOptions::new().read(true).write(write).open(path)
}

/// Strips `<header>\n<generation>\n` and the final newline, leaving the state word.
/// `None` for anything else, including a foreign generation.
pub(crate) fn state<'a>(bytes: &'a [u8], header: &str, generation: &str) -> Option<&'a str> {
    std::str::from_utf8(bytes)
        .ok()?
        .strip_prefix(header)?
        .strip_prefix('\n')?
        .strip_prefix(generation)?
        .strip_prefix('\n')?
        .strip_suffix('\n')
}

/// Whether durable replacement flushes the file and the directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Durability {
    /// `sync_all` on the temporary file before the rename, then the directory after it.
    Flushed,
    /// Rename only: a crash may lose the new content, never tear it.
    Unflushed,
}

/// Replaces `name` with `bytes` through `<name>.tmp`. The caller holds `io.lock`, so no
/// two engine writers share the temporary name. A stale temporary file is removed first
/// and the new one is created exclusively, so a planted link is never followed.
pub(crate) fn replace(root: &Path, name: &str, bytes: &[u8], durability: Durability) -> Result<()> {
    let temporary = root.join(format!("{name}.tmp"));
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let created = file.metadata()?;
    let listed = fs::symlink_metadata(&temporary)?;
    if !created.is_file() || !listed.is_file() || !same_file(&listed, &created) {
        return Err(Error::Replaced);
    }
    file.write_all(bytes)?;
    if durability == Durability::Flushed {
        // On Apple platforms std issues `F_FULLFSYNC` for `sync_all`.
        file.sync_all()?;
    }
    drop(file);
    fs::rename(&temporary, root.join(name))?;
    if durability == Durability::Flushed {
        sync_dir(root)?;
    }
    Ok(())
}

/// Flushes a directory entry change. `EINVAL` (a filesystem that cannot sync a
/// directory) is tolerated, as SQLite tolerates it.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> Result<()> {
    match fs::File::open(dir)?.sync_all() {
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
        other => Ok(other?),
    }
}

/// Non-unix roots never reach here: `Binding` refuses them first.
#[cfg(not(unix))]
pub(crate) fn sync_dir(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn same_file(listed: &fs::Metadata, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    listed.dev() == opened.dev() && listed.ino() == opened.ino()
}

#[cfg(not(unix))]
fn same_file(_: &fs::Metadata, _: &fs::Metadata) -> bool {
    true
}
