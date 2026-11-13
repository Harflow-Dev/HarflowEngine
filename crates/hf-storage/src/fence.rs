//! Verification fence: `store.fence` in the state root.
//!
//! Content: `harflow-fence-1\n<generation>\nfailed\n`, at most 128 bytes. The verifier
//! writes it under `io.lock` when a whole-store check finds corruption (`verify.rs`);
//! clearing it belongs to the maintenance actor (E2-B). It is created lazily, never at
//! initialization. While it names the current generation every writer open, commit and
//! read refuses with `Error::Fenced`.
use crate::{Error, Result, small_file};
use std::path::Path;

const NAME: &str = "store.fence";
const HEADER: &str = "harflow-fence-1";
const BOUND: u64 = 128;

/// Absent passes; `failed` for `generation` is `Fenced`; a foreign generation, malformed
/// or oversized content, a non-regular file, or a regular file replaced by another after
/// `lstat` is `Incompatible`. A symlink swapped in after `lstat` fails the no-follow open
/// (`ELOOP`) and is `Io`. Never creates or writes a file.
pub(crate) fn check(root: &Path, generation: &str) -> Result<()> {
    let Some(bytes) = small_file::read(root, NAME, BOUND)? else {
        return Ok(());
    };
    match small_file::state(&bytes, HEADER, generation) {
        Some("failed") => Err(Error::Fenced),
        _ => Err(Error::Incompatible),
    }
}

/// Writes the fence for `generation` durably (file and directory flushed) and reads it
/// back. The caller holds `io.lock`.
pub(crate) fn write(root: &Path, generation: &str) -> Result<()> {
    let bytes = format!("{HEADER}\n{generation}\nfailed\n");
    small_file::replace(
        root,
        NAME,
        bytes.as_bytes(),
        small_file::Durability::Flushed,
    )?;
    match check(root, generation) {
        Err(Error::Fenced) => Ok(()),
        Err(error) => Err(error),
        Ok(()) => Err(Error::Replaced),
    }
}
