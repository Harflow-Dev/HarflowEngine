//! Verification fence: `store.fence` in the state root.
//!
//! Content: `harflow-fence-1\n<generation>\n<state>\n`, at most 128 bytes, where the state
//! is `failed` (the verifier found corruption, `verify.rs`) or `commit_lost` (a
//! writer-role path found the database below `store.watermark`). It is written under
//! `io.lock`, lazily, never at initialization, and only when no fence exists: the first
//! state to land stays, so `failed` and `commit_lost` never overwrite each other. Probe
//! and readers never write it; clearing it belongs to the maintenance actor. While it
//! names the current generation every writer open, commit and read refuses: `failed` as
//! `Error::Fenced`, `commit_lost` as `Error::CommitLost`.
use crate::{Binding, Error, Result, small_file, wal};
use std::path::Path;

const NAME: &str = "store.fence";
const HEADER: &str = "harflow-fence-1";
const BOUND: u64 = 128;

/// The two fence states.
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Failed,
    CommitLost,
}
impl State {
    fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::CommitLost => "commit_lost",
        }
    }
}

/// Absent passes; `failed` for `generation` is `Fenced`; `commit_lost` for `generation` is
/// `CommitLost`; a foreign generation, malformed or oversized content, a non-regular
/// file, or a regular file replaced by another after `lstat` is `Incompatible`. A symlink
/// swapped in after `lstat` fails the no-follow open (`ELOOP`) and is `Io`. Never creates
/// or writes a file.
pub(crate) fn check(root: &Path, generation: &str) -> Result<()> {
    let Some(bytes) = small_file::read(root, NAME, BOUND)? else {
        return Ok(());
    };
    match small_file::state(&bytes, HEADER, generation) {
        Some("failed") => Err(Error::Fenced),
        Some("commit_lost") => Err(Error::CommitLost),
        _ => Err(Error::Incompatible),
    }
}

/// Whether a fence of either state exists for `generation`; `Ok(false)` when none does.
/// Any other fence content, or a failure to read it, is returned as its error.
fn exists(root: &Path, generation: &str) -> Result<bool> {
    match check(root, generation) {
        Ok(()) => Ok(false),
        Err(Error::Fenced | Error::CommitLost) => Ok(true),
        Err(error) => Err(error),
    }
}

/// Writes the `failed` fence for `generation` durably (file and directory flushed) and
/// reads it back, unless a fence of either state already exists for it, which is left in
/// place. The caller holds `io.lock`.
pub(crate) fn write(root: &Path, generation: &str) -> Result<()> {
    write_state(root, generation, State::Failed)
}

/// Records a `CommitLost` found by a writer-role path that holds `io.lock`: writes the
/// `commit_lost` fence for the generation the marker names, when it is still `generation`
/// and no fence exists. Returns the error to report, always `CommitLost`: a failure to
/// record the fence is not reported, and leaves the next writer-role path to record it.
pub(crate) fn record_commit_lost(binding: &Binding, generation: &str) -> Error {
    let current = wal::marker(binding).is_ok_and(|marker| marker.generation == generation);
    if current {
        let _ = write_state(binding.root(), generation, State::CommitLost);
    }
    Error::CommitLost
}

/// Maps `CommitLost` through `record_commit_lost`; any other error is returned unchanged.
pub(crate) fn on_commit_lost(binding: &Binding, generation: &str, error: Error) -> Error {
    match error {
        Error::CommitLost => record_commit_lost(binding, generation),
        other => other,
    }
}

fn write_state(root: &Path, generation: &str, state: State) -> Result<()> {
    if exists(root, generation)? {
        return Ok(());
    }
    let bytes = format!("{HEADER}\n{generation}\n{}\n", state.as_str());
    small_file::replace(
        root,
        NAME,
        bytes.as_bytes(),
        small_file::Durability::Flushed,
    )?;
    match (state, check(root, generation)) {
        (State::Failed, Err(Error::Fenced)) | (State::CommitLost, Err(Error::CommitLost)) => Ok(()),
        (_, Err(error)) => Err(error),
        (_, Ok(())) => Err(Error::Replaced),
    }
}
