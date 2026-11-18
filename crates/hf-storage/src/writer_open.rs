//! Writer-side entry points: `Store::open`, `Store::recover` and `probe`.
//!
//! Each qualifies the existing store in place (`qualify.rs`) and never copies it.
//! `open` verifies marker, header, journal, `-wal` and identity only; the whole-store
//! `quick_check` belongs to `recover` and `probe` (owner decision A2). Each first passes
//! the state-recovery gate (`recovery_gate.rs`). `open` and `recover` record a `CommitLost`
//! they find as the `commit_lost` fence; `probe` never writes one.
use crate::{
    Checkpoint, CheckpointPolicy, Error, Identity, Limits, Maintenance, Result, Store,
    WriterOptions,
    binding::Binding,
    checkpoint::checkpoint,
    connection::{self, open_source},
    fence,
    qualify::{self, Scan},
    recovery_gate,
};
use hf_application::lease::{LeaseMode, LeasePort};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

impl Store {
    /// `open_with(.., WriterOptions::suppressed())`: no checkpoint of any kind. A shorthand
    /// for tests; production callers name their policy through `open_with`.
    pub fn open(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
    ) -> Result<Self> {
        Self::open_with(root, limits, deadline, leases, WriterOptions::suppressed())
    }
    /// Opens a writer handle with an explicit checkpoint policy. Under `Owner`, the
    /// connection gets `journal_size_limit`. Under `Maintenance::AfterCommit` one PASSIVE
    /// checkpoint runs under `io.lock` after qualification and `Store::write` then calls
    /// `maintain_wal`; under `Maintenance::Idle` open performs validation only and the
    /// holder's idle loop maintains (`wal_idle.rs`).
    pub fn open_with(
        root: impl AsRef<Path>,
        limits: Limits,
        deadline: Instant,
        leases: Arc<dyn LeasePort>,
        options: WriterOptions,
    ) -> Result<Self> {
        connection::check_deadline(deadline)?;
        let binding = recovery_gate::open_existing(root.as_ref(), leases)?;
        let claim = binding.claim(LeaseMode::Shared, deadline)?;
        // Refusals in read-session order come before `io()`, whose full sidecar check
        // would otherwise report a defective `-wal` as `InvalidRoot`.
        let (expected, _) = qualify::precheck(&binding)?;
        let _io = binding.io(deadline)?;
        // A database below the watermark is fenced `commit_lost` under `io.lock`.
        let (identity, wal) = qualify::in_place(&binding, deadline, Scan::Identity)
            .map_err(|error| fence::on_commit_lost(&binding, &expected.generation, error))?;
        let connection = open_source(&binding, &identity, deadline, options.checkpoint)?;
        if options.checkpoint == CheckpointPolicy::Owner
            && options.maintenance == Maintenance::AfterCommit
        {
            Self::passive_at_open(&connection, deadline)?;
            binding.check()?;
        }
        qualify::recheck_wal(&binding, wal)?;
        Ok(Self {
            connection: Mutex::new(connection),
            binding,
            identity,
            limits,
            options,
            poisoned: std::sync::atomic::AtomicBool::new(false),
            idle: Mutex::default(),
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
        let binding = recovery_gate::open_existing(root.as_ref(), leases)?;
        let _claim = binding.claim(LeaseMode::Exclusive, deadline)?;
        // Refusals in read-session order come before `io()`, whose full sidecar check
        // would otherwise report a defective `-wal` as `InvalidRoot`.
        let (expected, _) = qualify::precheck(&binding)?;
        let _io = binding.io(deadline)?;
        // A database below the watermark is fenced `commit_lost` under `io.lock`.
        let (identity, wal) = qualify::in_place(&binding, deadline, Scan::QuickCheck)
            .map_err(|error| fence::on_commit_lost(&binding, &expected.generation, error))?;
        let connection = open_source(&binding, &identity, deadline, CheckpointPolicy::Suppressed)?;
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
    let binding = recovery_gate::open_existing(root.as_ref(), leases)?;
    let _claim = binding.claim(LeaseMode::Shared, deadline)?;
    let (identity, _) = qualify::in_place(&binding, deadline, Scan::QuickCheck)?;
    binding.check()?;
    Ok(Some(identity))
}
