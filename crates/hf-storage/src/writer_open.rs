//! Writer-side entry points: `Store::open`, `Store::recover` and `probe`.
//!
//! Each qualifies the existing store in place (`qualify.rs`) and never copies it.
//! `open` verifies marker, header, journal, `-wal` and identity only; the whole-store
//! `quick_check` belongs to `recover` and `probe` (owner decision A2).
use crate::{
    Checkpoint, Error, Identity, Limits, Result, Store,
    binding::Binding,
    checkpoint::checkpoint,
    connection::{self, open_source},
    qualify::{self, Scan},
};
use hf_application::lease::{LeaseMode, LeasePort};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

impl Store {
    pub fn open(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true, leases)?;
        let claim = binding.claim(LeaseMode::Shared, deadline)?;
        // Refusals in read-session order come before `io()`, whose full sidecar check
        // would otherwise report a defective `-wal` as `InvalidRoot`.
        qualify::precheck(&binding)?;
        let _io = binding.io(deadline)?;
        let (identity, wal) = qualify::in_place(&binding, deadline, Scan::Identity)?;
        let connection = open_source(&binding, &identity, deadline)?;
        qualify::recheck_wal(&binding, wal)?;
        Ok(Self {
            connection: Mutex::new(connection),
            binding,
            identity,
            limits,
            _claim: claim,
        })
    }
    /// Explicit maintenance refuses while any normal handle exists. No schema migration or reset.
    pub fn recover(
        root: impl AsRef<Path>,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Checkpoint> {
        connection::check_deadline(deadline)?;
        let binding = Binding::capture(root.as_ref(), true, leases)?;
        let _claim = binding.claim(LeaseMode::Exclusive, deadline)?;
        // Refusals in read-session order come before `io()`, whose full sidecar check
        // would otherwise report a defective `-wal` as `InvalidRoot`.
        qualify::precheck(&binding)?;
        let _io = binding.io(deadline)?;
        let (identity, wal) = qualify::in_place(&binding, deadline, Scan::QuickCheck)?;
        let connection = open_source(&binding, &identity, deadline)?;
        qualify::recheck_wal(&binding, wal)?;
        let result = checkpoint(&connection, true, deadline)?;
        binding.check()?;
        if !result.complete {
            return Err(Error::Busy);
        }
        Ok(result)
    }
}

/// Missing roots create nothing; existing roots are qualified in place and read-only.
/// Uses a one-second deadline; see `probe_with_deadline`.
pub fn probe(root: impl AsRef<Path>, leases: Arc<dyn LeasePort>) -> Result<Option<Identity>> {
    probe_with_deadline(root, Instant::now() + Duration::from_secs(1), leases)
}

/// As `probe`, bounded by `deadline`. The scan runs without `io.lock`: a read-only
/// snapshot is consistent, so a probe never delays a commit. It may create or rewrite
/// `-shm`, never main or WAL content, and never creates `-wal`.
pub fn probe_with_deadline(
    root: impl AsRef<Path>,
    deadline: Instant,
    leases: Arc<dyn LeasePort>,
) -> Result<Option<Identity>> {
    connection::check_deadline(deadline)?;
    Binding::validate_probe_path(root.as_ref())?;
    if !root.as_ref().try_exists()? {
        return Ok(None);
    }
    let binding = Binding::capture(root.as_ref(), true, leases)?;
    let _claim = binding.claim(LeaseMode::Shared, deadline)?;
    let (identity, _) = qualify::in_place(&binding, deadline, Scan::QuickCheck)?;
    binding.check()?;
    Ok(Some(identity))
}
