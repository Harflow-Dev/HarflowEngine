//! The state-recovery gate: `store.recovery` in the state root (format in
//! `hf_domain::ctl::state_recovery`).
//!
//! A `pending` record refuses every store opener as `Error::RecoveryInterrupted` at three
//! points, so no opener reaches a marker, database or sidecar check first while a
//! recovery moves the store out of the root:
//! 1. lease-free, before the opener identifies the root (`open_existing`); a failure to
//!    identify it is then mapped through one more lease-free read (`after_failure`),
//!    covering a recovery that moved the marker or database between that read and the
//!    identification;
//! 2. in `Binding::claim`, right after the lease (`check`);
//! 3. in `ReadSession::open`, right after its own lease (`check`).
//!
//! `committed` and an absent record pass. A malformed, oversized or non-regular record is
//! `Incompatible`. Nothing here writes except `write_recovery_record`.
use crate::{Binding, Error, Result, small_file};
use hf_application::lease::LeasePort;
use hf_domain::ctl::state_recovery::{RECORD_BOUND, RECORD_NAME, StoreRecovery};
use std::{path::Path, sync::Arc};

/// Extra reads after the record was replaced or removed between `lstat` and the open, as a
/// recovery's atomic rename of a new record does.
pub(crate) const REPLACED_RETRIES: u32 = 3;

/// The decoded record, or `None` when it is absent. A malformed, oversized or non-regular
/// record is `Incompatible`; a symlink swapped in after `lstat` is `Io` (`ELOOP`). A record
/// replaced or removed between `lstat` and the open is read again, up to
/// `REPLACED_RETRIES` times; one still changing after that is `Incompatible` (replaced) or
/// `Io` (removed).
pub(crate) fn read(root: &Path) -> Result<Option<StoreRecovery>> {
    read_with(|| small_file::read_listed(root, RECORD_NAME, RECORD_BOUND))
}

/// `read` over an injected listing source, called once per attempt.
pub(crate) fn read_with(
    mut list: impl FnMut() -> Result<small_file::Listed>,
) -> Result<Option<StoreRecovery>> {
    let mut attempt = 0;
    let content = loop {
        match list()? {
            small_file::Listed::Content(content) => break content,
            _ if attempt < REPLACED_RETRIES => attempt += 1,
            small_file::Listed::Replaced => return Err(Error::Incompatible),
            small_file::Listed::Removed(error) => return Err(error.into()),
        }
    };
    let bytes = match content {
        small_file::Content::Absent => return Ok(None),
        small_file::Content::Oversized => return Err(Error::Incompatible),
        small_file::Content::Bytes(bytes) => bytes,
    };
    StoreRecovery::decode(&bytes)
        .map(Some)
        .ok_or(Error::Incompatible)
}

/// The record of a `read_with` source: `pending` is `RecoveryInterrupted`, as in `check`.
#[cfg(test)]
pub(crate) fn check_with(list: impl FnMut() -> Result<small_file::Listed>) -> Result<()> {
    match read_with(list)? {
        Some(record) if record.is_pending() => Err(Error::RecoveryInterrupted),
        _ => Ok(()),
    }
}

/// Gate points 2 and 3, under the opener's lease: `pending` is `RecoveryInterrupted`;
/// every read failure is returned.
pub(crate) fn check(root: &Path) -> Result<()> {
    match read(root)? {
        Some(record) if record.is_pending() => Err(Error::RecoveryInterrupted),
        _ => Ok(()),
    }
}

/// Whether the lease-free points read the record at all: every existing component of the
/// root path must be a directory and not a symlink (`Binding::validate_probe_path`, which
/// passes for a missing root). Any other path is left to the opener's identification, so
/// every opener refuses a symlinked ancestor or a non-directory root as it does today.
fn path_is_plain(root: &Path) -> bool {
    Binding::validate_probe_path(root).is_ok()
}

/// Gate point 1, lease-free. As `check`, except that it reads nothing for a path
/// `path_is_plain` rejects, and an I/O failure reading the record (a missing root, an
/// unreadable entry) is left to the identification that follows, so the opener's existing
/// refusal for such a root is unchanged; gate point 2 reads the record again under the
/// lease.
pub(crate) fn check_lease_free(root: &Path) -> Result<()> {
    if !path_is_plain(root) {
        return Ok(());
    }
    match check(root) {
        Err(Error::Io(_)) => Ok(()),
        other => other,
    }
}

/// Maps a failure to identify or qualify the root before the opener's lease. For a path
/// `path_is_plain` accepts, `Io`, `InvalidRoot` and `Incompatible` are mapped through one
/// more lease-free read of the record: `pending` is `RecoveryInterrupted`; an I/O failure
/// reading the record of an existing root directory is that failure, as gate point 2
/// would report it; any other read result keeps the original error. Every other error,
/// and every error for another path, is returned unchanged.
pub(crate) fn after_failure(root: &Path, error: Error) -> Error {
    if !matches!(
        error,
        Error::Io(_) | Error::InvalidRoot | Error::Incompatible
    ) || !path_is_plain(root)
    {
        return error;
    }
    match read(root) {
        Ok(Some(record)) if record.is_pending() => Error::RecoveryInterrupted,
        Err(record_error @ Error::Io(_)) if is_directory(root) => record_error,
        _ => error,
    }
}

fn is_directory(root: &Path) -> bool {
    std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.is_dir())
}

/// Gate point 1 and the identification of an existing root with its sidecar, marker and
/// header checks (`Binding::capture`), with a failure mapped through `after_failure`.
pub(crate) fn open_existing(root: &Path, leases: Arc<dyn LeasePort>) -> Result<Binding> {
    check_lease_free(root)?;
    capture_after_gate(root, leases)
}

/// The identification step of `open_existing`, after its lease-free read:
/// `Binding::capture` with a failure mapped through `after_failure`.
pub(crate) fn capture_after_gate(root: &Path, leases: Arc<dyn LeasePort>) -> Result<Binding> {
    Binding::capture(root, true, leases).map_err(|error| after_failure(root, error))
}

/// Replaces `store.recovery` with `record` durably: a fresh temporary file, flushed, renamed
/// over the record, then the directory flushed. The caller excludes every other writer of
/// the record (the maintenance session's exclusive locks).
pub fn write_recovery_record(root: &Path, record: &StoreRecovery) -> Result<()> {
    small_file::replace(
        root,
        RECORD_NAME,
        record.encode().as_bytes(),
        small_file::Durability::Flushed,
    )
}
