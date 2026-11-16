//! A state-recovery proposal and the inventory it commits to.
//!
//! A proposal names the root, the engine that made it and a digest of the root's
//! inventory. Its confirmation token is the canonical encoding of these values
//! (`hf-interfaces`), so a changed root or a token from another engine is stale.
use std::collections::BTreeMap;

/// Upper bound on the entries one inventory may hold.
pub const INVENTORY_BOUND: usize = 100_000;
/// Upper bound on a confirmation token's length in characters.
pub const TOKEN_BOUND: usize = 8192;
/// Phase of a proposal that no recovery has started.
pub const PHASE_PROPOSED: &str = "proposed";

/// Root entries that belong to the engine's own locking and recovery bookkeeping and are
/// never archived: the two lock families, the recovery journal and the recovery record
/// with their temporaries, and the shared-memory sidecar, which readers rewrite.
pub const INVENTORY_EXCLUDED: [&str; 8] = [
    ".harflow-store.lock",
    ".harflow-recovery.json",
    ".harflow-recovery.json.pending",
    "store.lock",
    "io.lock",
    "store.recovery",
    "store.recovery.tmp",
    "store.sqlite3-shm",
];

/// Top-level directories that only the reference engine fills; a root holding data in
/// one of them carries state this engine cannot recover.
pub const REFERENCE_STATE_DIRECTORIES: [&str; 3] = ["tasks", "workflow-runs", "workflow-leases"];

/// Device and inode of a filesystem object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    pub dev: i64,
    pub ino: u64,
}

/// What the confirmation token commits to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryProposal {
    pub state_root: String,
    pub engine_version: String,
    pub root_identity: FileIdentity,
    /// The stable lock file, `None` when the root has none.
    pub lock_identity: Option<FileIdentity>,
    pub inventory_sha256: String,
    pub operation_id: String,
    /// SHA-256 of the earlier recovery journal's bytes, when one is present.
    pub prior_journal_sha256: Option<String>,
}
impl RecoveryProposal {
    /// The backup sibling directory of this operation.
    pub fn backup_path(&self) -> String {
        let mut path = self.state_root.clone();
        path.push_str(".harflow-backup-");
        path.push_str(&self.operation_id);
        path
    }
}

/// A proposal as `state-inspect` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryOffer {
    pub proposal: RecoveryProposal,
    pub phase: String,
    pub resumable: bool,
}

/// The attributes the inventory records for every entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryStat {
    pub dev: i64,
    pub ino: u64,
    pub mode: u32,
    pub size: i64,
    pub mtime_ns: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryBody {
    File,
    /// A symbolic link, recorded and never followed.
    Link(String),
    Directory(BTreeMap<String, InventoryEntry>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryEntry {
    pub stat: EntryStat,
    pub body: EntryBody,
}

/// A complete, unchanged walk of a state root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub root: FileIdentity,
    pub lock: Option<FileIdentity>,
    pub entries: BTreeMap<String, InventoryEntry>,
}
impl Inventory {
    /// Whether the root carries data in a directory only the reference engine fills.
    pub fn has_reference_state(&self) -> bool {
        REFERENCE_STATE_DIRECTORIES
            .iter()
            .any(|name| match self.entries.get(*name) {
                Some(InventoryEntry {
                    body: EntryBody::Directory(children),
                    ..
                }) => !children.is_empty(),
                _ => false,
            })
    }
}

/// Why a walk produced no inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryFailure {
    /// More entries than `INVENTORY_BOUND`, or a tree nested too deeply to walk.
    TooLarge,
    /// A socket, FIFO or device: the root cannot be archived safely.
    Special,
    /// An entry or the root changed while it was walked.
    Changed,
    /// An entry could not be read, or its name or link target is not UTF-8.
    Unreadable,
    /// This build cannot walk a root.
    Unsupported,
}
