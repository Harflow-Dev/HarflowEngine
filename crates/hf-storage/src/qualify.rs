//! In-place qualification of an existing store: no copy, no scratch directory.
//!
//! The check opens the source database read-only with checkpoints suppressed, so
//! it never writes main or WAL content and never removes `-wal`. A missing `-wal`
//! is refused before SQLite is opened, because SQLite would create an empty one.
use crate::{
    Error, Identity, Result, binding::Binding, connection, fence, read_session::open_read_only,
    scan, wal, wal_acceptance, watermark,
};
use std::{fs, time::Instant};

/// How much of the store `in_place` verifies beyond marker, header and identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scan {
    /// Marker, header, journal, `-wal` and SQL identity only.
    Identity,
    /// Also one bounded `PRAGMA quick_check(1)` over the whole store.
    QuickCheck,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalStamp {
    device: u64,
    inode: u64,
}

/// Marker, header, rollback journal, `-wal`, acceptance record, fence, watermark, in that
/// order. Returns the marker identity and the watermark sequence.
pub(crate) fn precheck(binding: &Binding) -> Result<(Identity, u64)> {
    binding.check_sidecars(false)?;
    let expected = wal::marker(binding)?;
    connection::qualify_header(&binding.database())?;
    wal::reject_nonempty_journal(binding)?;
    require_wal(binding)?;
    wal_acceptance::check(binding.root(), &expected.generation)?;
    fence::check(binding.root(), &expected.generation)?;
    let mark = watermark::read(binding.root(), &expected.generation)?;
    binding.check()?;
    Ok((expected, mark))
}

/// Opens the source read-only, checks identity against the marker, refuses a database
/// sequence below the watermark (`CommitLost`; the file is read before the snapshot) and
/// optionally scans. The scan connection is dropped before this returns.
pub(crate) fn in_place(
    binding: &Binding,
    deadline: Instant,
    scan: Scan,
) -> Result<(Identity, WalStamp)> {
    connection::check_deadline(deadline)?;
    let (expected, mark) = precheck(binding)?;
    let stamp = wal_stamp(binding)?;
    {
        let connection = open_read_only(binding, deadline)?;
        if connection::identity(&connection, deadline).map_err(scan::corruption)? != expected {
            return Err(Error::Incompatible);
        }
        watermark::not_ahead(mark, &connection).map_err(scan::corruption)?;
        if scan == Scan::QuickCheck {
            scan::quick_check(&connection, deadline)?;
        }
    }
    binding.check()?;
    recheck_wal(binding, stamp)?;
    if wal::marker(binding)? != expected {
        return Err(Error::Replaced);
    }
    wal_acceptance::check(binding.root(), &expected.generation)?;
    fence::check(binding.root(), &expected.generation)?;
    // Presence only: the sequences were compared on the snapshot above.
    watermark::present(binding.root())?;
    connection::check_deadline(deadline)?;
    Ok((expected, stamp))
}

/// `-wal` must still be the file `precheck` saw; missing or swapped is `Replaced`.
pub(crate) fn recheck_wal(binding: &Binding, stamp: WalStamp) -> Result<()> {
    match wal_stamp(binding) {
        Ok(now) if now == stamp => Ok(()),
        Ok(_) => Err(Error::Replaced),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(Error::Replaced)
        }
        Err(error) => Err(error),
    }
}

/// `lstat` only: a missing or non-regular `-wal` is not a legitimate engine state.
fn require_wal(binding: &Binding) -> Result<()> {
    match fs::symlink_metadata(binding.root().join("store.sqlite3-wal")) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(Error::WalMissing),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::WalMissing),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
pub(crate) fn wal_stamp(binding: &Binding) -> Result<WalStamp> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(binding.root().join("store.sqlite3-wal"))?;
    Ok(WalStamp {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Non-unix roots never reach here: `Binding` refuses them first.
#[cfg(not(unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalStamp;

#[cfg(not(unix))]
pub(crate) fn wal_stamp(_: &Binding) -> Result<WalStamp> {
    Ok(WalStamp)
}
