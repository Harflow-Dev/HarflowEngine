//! The port of the pending launch reads, answered from the one snapshot, and the bounds of
//! one request.
use super::StoreFailure;
use hf_domain::{
    ctl::pending::{LaunchFacts, MAX_ENTRIES, MAX_NAME_BYTES, MAX_READ_BYTES},
    task_read::TaskId,
};
use std::cell::Cell;

/// The bounds of one request. The defaults are the contract's; a test seam may shrink them,
/// and no public flag exposes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingLimits {
    /// Launch rows in the namespace.
    pub entries: usize,
    /// Bytes of receipt filenames in the namespace.
    pub name_bytes: usize,
    /// Bytes one request may charge to its read budget.
    pub read_bytes: u64,
}

impl Default for PendingLimits {
    fn default() -> Self {
        Self {
            entries: MAX_ENTRIES,
            name_bytes: MAX_NAME_BYTES,
            read_bytes: MAX_READ_BYTES,
        }
    }
}

/// The bytes one request may read: the caller check and every examined row charge it.
pub struct PendingBudget {
    limit: u64,
    used: Cell<u64>,
}

impl PendingBudget {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            used: Cell::new(0),
        }
    }

    /// Charges `bytes`; `false` when they do not fit, which spends the budget.
    pub fn charge(&self, bytes: u64) -> bool {
        let left = self.limit.saturating_sub(self.used.get());
        if bytes > left {
            self.used.set(self.limit);
            return false;
        }
        self.used.set(self.used.get() + bytes);
        true
    }
}

/// What a cursor is bound to besides the namespace: the selected root and the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingBinding {
    /// The digest of the selected state root.
    pub root: String,
    /// The store identity, as four whole numbers.
    pub identity: [u64; 4],
}

/// The launch namespace: every receipt name, ordered by filename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Namespace {
    /// SHA-256 over the sorted filenames, each followed by a NUL.
    pub revision: String,
    /// Names that are not task identities.
    pub invalid: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceFailure {
    /// More entries or name bytes than the bound allows.
    Bound,
    Store(StoreFailure),
}

impl From<StoreFailure> for NamespaceFailure {
    fn from(failure: StoreFailure) -> Self {
        Self::Store(failure)
    }
}

pub trait PendingPort {
    fn pending_binding(&self, root: &str) -> Result<PendingBinding, StoreFailure>;
    /// One pass over every ordinary record. `false` when any of them cannot be qualified or
    /// the budget runs out, which makes the caller undecidable.
    fn candidates_checked(
        &self,
        budget: &PendingBudget,
        limits: &PendingLimits,
    ) -> Result<bool, StoreFailure>;
    fn pending_namespace(&self, limits: &PendingLimits) -> Result<Namespace, NamespaceFailure>;
    /// Whether `id` is a receipt name of the namespace.
    fn launch_name_exists(&self, id: &str) -> Result<bool, StoreFailure>;
    /// The id of the first launch row, named by a task identity, whose filename sorts after
    /// that of `after`. Only the name is read.
    fn next_launch_name(&self, after: Option<&str>) -> Result<Option<String>, StoreFailure>;
    /// The raw stored size of the ordinary record `id` (its header and status documents), or
    /// `None` without a record. Nothing of the record is decoded.
    fn record_size(&self, id: &TaskId) -> Result<Option<u64>, StoreFailure>;
    /// The launch row of `id`; a bare record when only an ordinary record has the id; `None`
    /// when nothing has it.
    fn pending_row(&self, id: &TaskId) -> Result<Option<LaunchFacts>, StoreFailure>;
}
