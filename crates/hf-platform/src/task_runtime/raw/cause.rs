//! The private retained-cause record of an owner that retains after its writer started.
//!
//! One bounded, best-effort write through descriptors: every component is opened without
//! following symlinks and checked for owner and mode, missing directories are created
//! 0700 with their parent flushed, and the file is created exclusively with mode 0600.
use super::{files::valid_name, open_private_root, refused};
use crate::durability::FullFlush;
use hf_application::durability::FlushPort;
use hf_domain::{capture::FileIdentity, task_lifecycle::LifecycleBinding};
use rustix::fs::{Mode, OFlags, mkdirat, openat};
use std::{
    fs::File,
    io,
    os::{fd::AsFd, unix::fs::MetadataExt},
    path::Path,
};

pub const CAUSE_FILE: &str = "owner-retained.cause";
/// The largest record, including its terminator.
pub const CAUSE_LIMIT: usize = 4096;
const TERMINATOR: &[u8] = b"end\n";

/// Writes `record` once to the attempt's private payload directory under `root`. The root
/// must still be the private directory with `identity`. The record must end with the
/// `end` line and stay within `CAUSE_LIMIT`. An existing file is never replaced.
pub fn write_retained_cause(
    root: &Path,
    identity: &FileIdentity,
    binding: &LifecycleBinding,
    record: &[u8],
) -> io::Result<()> {
    write_retained_cause_capped(root, identity, binding, record, record.len())
}
/// Fault injection only: as `write_retained_cause`, but the single write transfers at most `cap` bytes. A short
/// transfer is an error and leaves the file as written, without a retry.
#[doc(hidden)]
pub fn write_retained_cause_capped(
    root: &Path,
    identity: &FileIdentity,
    binding: &LifecycleBinding,
    record: &[u8],
    cap: usize,
) -> io::Result<()> {
    if record.len() > CAUSE_LIMIT || !record.ends_with(TERMINATOR) {
        return Err(refused());
    }
    let private = open_private_root(root)?;
    if private.identity() != identity {
        return Err(refused());
    }
    let mut dir = private.file().try_clone()?;
    for part in [
        "private-payload",
        binding.generation.as_str(),
        binding.task.as_str(),
        binding.attempt.as_str(),
    ] {
        dir = descend(&dir, part)?;
    }
    let fd = openat(
        dir.as_fd(),
        CAUSE_FILE,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(io::Error::from)?;
    let file = File::from(fd);
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 || m.uid() != nix::unistd::getuid().as_raw() {
        return Err(refused());
    }
    let wrote =
        rustix::io::write(&file, &record[..cap.min(record.len())]).map_err(io::Error::from)?;
    if wrote != record.len() {
        return Err(refused());
    }
    FullFlush.flush(&file)?;
    FullFlush.flush(&dir)
}
/// Opens or creates one directory component below `parent`, validating its owner and mode.
fn descend(parent: &File, part: &str) -> io::Result<File> {
    if !valid_name(part) {
        return Err(refused());
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = match openat(parent.as_fd(), part, flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            mkdirat(parent.as_fd(), part, Mode::from_raw_mode(0o700)).map_err(io::Error::from)?;
            FullFlush.flush(parent)?;
            openat(parent.as_fd(), part, flags, Mode::empty()).map_err(io::Error::from)?
        }
        Err(error) => return Err(error.into()),
    };
    let file = File::from(fd);
    let m = file.metadata()?;
    if !m.is_dir() || m.uid() != nix::unistd::getuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(refused());
    }
    Ok(file)
}
