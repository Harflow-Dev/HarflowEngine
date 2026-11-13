//! Lost-WAL acceptance record: `store.wal-acceptance` in the state root.
//!
//! Content: `harflow-wal-acceptance-1\n<generation>\n<pending|accepted|failed>\n`,
//! at most 128 bytes. Only the maintenance actor writes it (E2-B); every opener and
//! reader checks it after `-wal`. Absent means no acceptance is in progress; only
//! `accepted` for the current generation lets the root be used.
use crate::{Error, Result, small_file};
use std::path::Path;

const NAME: &str = "store.wal-acceptance";
const HEADER: &str = "harflow-wal-acceptance-1";
const BOUND: u64 = 128;

/// Refuses a root whose acceptance record is not `accepted` for `generation`.
/// `pending` and `failed` read as a missing `-wal`; a foreign generation, malformed
/// or oversized content, a non-regular file, or a file that is not the one `lstat` listed (device/inode
/// mismatch after open) is `Incompatible`. It never creates
/// or writes a file.
pub(crate) fn check(root: &Path, generation: &str) -> Result<()> {
    let Some(bytes) = small_file::read(root, NAME, BOUND)? else {
        return Ok(());
    };
    match small_file::state(&bytes, HEADER, generation) {
        Some("accepted") => Ok(()),
        Some("pending" | "failed") => Err(Error::WalMissing),
        _ => Err(Error::Incompatible),
    }
}
