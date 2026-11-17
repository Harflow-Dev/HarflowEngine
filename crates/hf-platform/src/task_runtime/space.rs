//! Read-only free-space admission. This is not path or root authority.
use nix::sys::statvfs::statvfs;
use std::{collections::BTreeSet, fs, io, os::unix::fs::MetadataExt, path::Path, time::Instant};

pub const MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;

/// Check each distinct filesystem using an existing ancestor for future output paths.
/// Missing evidence or an expired observation refuses before launch preparation.
pub fn preflight(paths: &[&Path], deadline: Instant) -> io::Result<()> {
    if paths.is_empty() || paths.len() > 4 {
        return Err(io::Error::other("free-space path inventory refused"));
    }
    let mut devices = BTreeSet::new();
    for path in paths {
        timely(deadline)?;
        if !path.is_absolute() {
            return Err(io::Error::other("free-space path must be absolute"));
        }
        let existing = existing_ancestor(path, deadline)?;
        let before = fs::metadata(existing)?;
        if !devices.insert(before.dev()) {
            continue;
        }
        let space = statvfs(existing)?;
        timely(deadline)?;
        let available = u128::from(space.blocks_available())
            .checked_mul(u128::from(space.fragment_size()))
            .ok_or_else(|| io::Error::other("free-space count overflow"))?;
        if space.fragment_size() == 0 || available < u128::from(MIN_FREE_BYTES) {
            return Err(io::Error::other("at least 256 MiB free space is required"));
        }
        let after = fs::metadata(existing)?;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err(io::Error::other("free-space filesystem changed"));
        }
    }
    timely(deadline)
}

fn existing_ancestor(mut path: &Path, deadline: Instant) -> io::Result<&Path> {
    for _ in 0..256 {
        timely(deadline)?;
        match fs::metadata(path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                path = path.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("free-space ancestor bound exceeded"))
}

fn timely(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "free-space observation expired",
        ))
    } else {
        Ok(())
    }
}
