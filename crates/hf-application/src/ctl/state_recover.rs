//! Internal state recovery ports, qualified owner evidence and archive-only flow.
//! The flow stops at a pending BackedUp handoff; no public recovery caller exists.
pub mod binding;
pub mod evidence;
pub mod journal;

use std::{io, path::Path};

/// Filesystem effects of a state recovery that the storage adapter cannot perform itself.
pub trait RecoveryFsPort: Send + Sync {
    /// Creates `root/name` as a new regular file with mode 0600, failing on an existing
    /// entry of any kind and never following a final symbolic link. `Ok(true)` when this
    /// call created it; `Ok(false)` when the name already existed (a racing creator or an
    /// earlier attempt), which is not an error: the caller's identity checks decide.
    fn create_lock(&self, root: &Path, name: &str) -> io::Result<bool>;

    /// Bytes available to an unprivileged writer on the volume holding `path`.
    fn free_space(&self, path: &Path) -> io::Result<u64>;
}

pub mod flow;
