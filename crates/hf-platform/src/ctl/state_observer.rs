//! Read-only observation of a state root: link-aware stats and bounded reads.
//! It never creates, locks, writes, renames or opens a database.
//!
//! Reads go through a `RootHandle`. On macOS the handle is a no-follow directory
//! descriptor and every leaf is opened relative to it, so a swapped root or ancestor
//! cannot redirect a read and a swapped FIFO cannot block it. Other targets (Linux is
//! W02, Windows WIN02) re-open by path after a link-aware stat; on Unix they compare
//! file identity after opening, which narrows but does not close a replacement race.
use hf_application::ctl::state_inspect::StateObserverPort;
use hf_domain::ctl::{
    inspect::{FileState, RootFacts, RootStat},
    state_root::StateRoot,
};
use std::{fs, io::ErrorKind, path::Path};
#[cfg(target_os = "macos")]
mod descriptor;
#[cfg(not(target_os = "macos"))]
mod path_based;
#[cfg(target_os = "macos")]
use descriptor::RootHandle;
#[cfg(not(target_os = "macos"))]
use path_based::RootHandle;
const MARKER_BOUND: u64 = 16_384;
const IDENTITY_BOUND: u64 = 128;
const JOURNAL_BOUND: u64 = 16 * 1024 * 1024;
const HEADER_BYTES: u64 = 100;
const ENTRY_LIMIT: usize = 256;
/// Device and inode of the checked root, where the platform has them.
type FileId = Option<(u64, u64)>;
#[derive(Clone, Copy, Debug, Default)]
pub struct FsStateObserver;
impl StateObserverPort for FsStateObserver {
    fn observe(&self, root: &StateRoot) -> RootFacts {
        let path = Path::new(root.as_str());
        let (stat, id) = stat_root(path);
        let mut facts = RootFacts::without_files(stat);
        if stat != RootStat::Present {
            return facts;
        }
        let unreadable = || RootFacts::without_files(RootStat::Unreadable);
        let Some(handle) = RootHandle::open(path, id) else {
            return unreadable();
        };
        let Some((entries, truncated)) = handle.entries(ENTRY_LIMIT) else {
            return unreadable();
        };
        facts.entries = entries;
        facts.entries_truncated = truncated;
        facts.python_marker = handle.read(".harflow-store.json", MARKER_BOUND, true);
        facts.store_identity = handle.read("store.identity", IDENTITY_BOUND, true);
        facts.database_header = handle.read("store.sqlite3", HEADER_BYTES, false);
        facts.journal = handle.read(".harflow-recovery.json", JOURNAL_BOUND, true);
        facts.journal_pending = handle.read(".harflow-recovery.json.pending", JOURNAL_BOUND, true);
        // A stat of the name itself: the 256-entry listing cannot prove absence.
        facts.wal = handle.kind("store.sqlite3-wal");
        facts.wal_acceptance = handle.read("store.wal-acceptance", IDENTITY_BOUND, true);
        facts
    }
}
#[cfg(unix)]
fn file_id(metadata: &fs::Metadata) -> FileId {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn file_id(_: &fs::Metadata) -> FileId {
    None
}
/// A missing root is `Missing` whatever its ancestors are. An existing root must be a
/// plain directory under plain directories, must not be the filesystem root or the home
/// directory, and must share a device with its parent.
fn stat_root(path: &Path) -> (RootStat, FileId) {
    // The reference refuses these before it looks at the filesystem at all.
    if path.parent().is_none() || super::state_root::home_directory().as_deref() == Some(path) {
        return (RootStat::Unsafe, None);
    }
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return (RootStat::Missing, None),
        Err(_) => return (RootStat::Unreadable, None),
        Ok(metadata) if !metadata.file_type().is_dir() => return (RootStat::Unsafe, None),
        Ok(metadata) => metadata,
    };
    let id = file_id(&metadata);
    for (index, ancestor) in path.ancestors().skip(1).enumerate() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Err(_) => return (RootStat::Unreadable, id),
            Ok(parent) if !parent.file_type().is_dir() => return (RootStat::Unsafe, id),
            Ok(parent) if index == 0 && file_id(&parent).map(|p| p.0) != id.map(|r| r.0) => {
                return (RootStat::Unsafe, id);
            }
            Ok(_) => {}
        }
    }
    (RootStat::Present, id)
}
/// Reads at most `limit` bytes. With `strict`, a longer file is `Oversize`; otherwise
/// only the prefix is wanted.
fn finish_read(file: fs::File, size: u64, limit: u64, strict: bool) -> FileState {
    use std::io::Read;
    if strict && size > limit {
        return FileState::Oversize;
    }
    let mut bytes = Vec::new();
    if file.take(limit + 1).read_to_end(&mut bytes).is_err() {
        return FileState::Unreadable;
    }
    if strict && bytes.len() as u64 > limit {
        return FileState::Oversize;
    }
    bytes.truncate(limit as usize);
    FileState::Bytes(bytes)
}
