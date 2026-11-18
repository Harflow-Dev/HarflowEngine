//! Writer-side WAL ownership: checkpoint policy and post-commit maintenance.
//!
//! `Owner` bounds the `-wal` file: `journal_size_limit` 16 MiB and PASSIVE checkpoints. It
//! keeps both SQLite checkpoint suppressions (`wal_autocheckpoint=0`, no checkpoint on
//! close), never uses TRUNCATE (only `recover` does) and never RESTART, so `-wal` always
//! survives. `Suppressed` is the behaviour of the fixture seed, the host fixture modes and
//! the watch seam: nothing checkpoints, so seeded roots keep their nonempty `-wal`.
//!
//! Two maintenance modes exist under `Owner`. `Maintenance::AfterCommit` (`owner()`) runs an
//! unbounded open-time PASSIVE and a PASSIVE after a commit once the live log passes 8 MiB.
//! `Maintenance::Idle` (`owner_idle()`, the task writer) is driven by the holder's idle loop
//! through `maintain_wal`, copies on a transient connection bounded by a watchdog
//! (`wal_idle.rs`) and falls back to the commit path only above `HARD_LIVE_LOG`.
use crate::{
    Error, Result, Store,
    checkpoint::checkpoint,
    connection,
    wal_idle::{LogState, Origin},
    write_tx::Publish,
};
use hf_application::durability::FlushPort;
use std::{
    fs::{self, File},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

/// `PRAGMA journal_size_limit`: the next write after a complete checkpoint cuts `-wal` to this.
pub const JOURNAL_SIZE_LIMIT: u64 = 16 * 1024 * 1024;
/// A commit that leaves a live log longer than this is followed by a PASSIVE checkpoint.
pub const PASSIVE_AFTER: u64 = 8 * 1024 * 1024;
/// Upper bound on the maintenance budget after a commit. It bounds admission, `io.lock`
/// and SQLite busy waits, not the PASSIVE copy itself, which runs to completion.
pub const MAINTENANCE_BUDGET: Duration = Duration::from_millis(100);
/// `Idle` mode only: a commit that leaves a live log longer than this (and an uncheckpointed
/// backlog above `PASSIVE_AFTER`) runs one bounded PASSIVE on the commit path.
pub const HARD_LIVE_LOG: u64 = 2 * JOURNAL_SIZE_LIMIT;
/// `Idle` mode only: default and cap of `WriterOptions::passive_budget`, the time one PASSIVE
/// copy may run after `io.lock` is held. A third of the owner's 5 s acknowledgement deadline.
pub const PASSIVE_COPY_BUDGET: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointPolicy {
    /// This handle checkpoints and bounds `-wal`.
    Owner,
    /// Today's behaviour: no checkpoint of any kind.
    Suppressed,
}

/// Where `Owner` maintenance runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maintenance {
    /// After each commit, plus an open-time PASSIVE (`owner()`).
    AfterCommit,
    /// From the holder's idle loop through `Store::maintain_wal`; the commit path only
    /// falls back above `HARD_LIVE_LOG` (`owner_idle()`).
    Idle,
}

/// How a commit takes `io.lock`. Set only by the constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoWait {
    /// One attempt: a held lock is `Busy` at once.
    Immediate,
    /// Retry a busy lock before any effect until `IO_RESERVE` before the deadline.
    PollUntilDeadline,
}

/// Chosen by the caller of `Store::open_with`; `Store::open` is `WriterOptions::suppressed()`.
/// `flush` makes the commit watermark durable: it runs twice per commit, before the slot
/// write and after it. `maintenance` and `passive_budget` apply to `Owner` only;
/// `passive_budget` is capped at `PASSIVE_COPY_BUDGET` and applies to `Idle`.
#[derive(Debug, Clone)]
pub struct WriterOptions {
    pub checkpoint: CheckpointPolicy,
    pub flush: Arc<dyn FlushPort>,
    pub maintenance: Maintenance,
    pub passive_budget: Duration,
    pub(crate) io_wait: IoWait,
}

/// Default flush of this crate: `sync_all` on the open file. On Apple platforms std
/// issues `F_FULLFSYNC` for it; elsewhere it is `fsync`. Production callers inject the
/// platform port for their chosen durability instead (`hf_platform::durability`).
#[derive(Debug, Clone, Copy)]
pub struct SyncAllFlush;
impl FlushPort for SyncAllFlush {
    fn flush(&self, file: &File) -> std::io::Result<()> {
        file.sync_all()
    }
}

/// The flush `suppressed()` and `owner()` start with: `SyncAllFlush`, except in this
/// crate's own unit tests, which do not flush (real flushes belong to dedicated
/// durability tests).
#[cfg(not(test))]
fn default_flush() -> Arc<dyn FlushPort> {
    Arc::new(SyncAllFlush)
}
#[cfg(test)]
fn default_flush() -> Arc<dyn FlushPort> {
    Arc::new(UnitTestFlush)
}
#[cfg(test)]
#[derive(Debug)]
struct UnitTestFlush;
#[cfg(test)]
impl FlushPort for UnitTestFlush {
    fn flush(&self, _: &File) -> std::io::Result<()> {
        Ok(())
    }
}

impl WriterOptions {
    pub fn suppressed() -> Self {
        Self {
            checkpoint: CheckpointPolicy::Suppressed,
            flush: default_flush(),
            maintenance: Maintenance::AfterCommit,
            passive_budget: PASSIVE_COPY_BUDGET,
            io_wait: IoWait::Immediate,
        }
    }
    pub fn owner() -> Self {
        Self {
            checkpoint: CheckpointPolicy::Owner,
            ..Self::suppressed()
        }
    }
    /// `Owner` for the long-lived task writer: maintenance from its idle loop, no open-time
    /// PASSIVE, and a commit that waits out a busy `io.lock` before any effect.
    pub fn owner_idle() -> Self {
        Self {
            maintenance: Maintenance::Idle,
            io_wait: IoWait::PollUntilDeadline,
            ..Self::owner()
        }
    }
    /// The same policy with `flush` as the watermark flush.
    pub fn with_flush(self, flush: Arc<dyn FlushPort>) -> Self {
        Self { flush, ..self }
    }
}

/// What one `maintain_wal` call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalMaintenance {
    /// Suppressed policy, or the log not past `PASSIVE_AFTER`.
    Idle,
    /// The connection, `io.lock` or checkpoint lock was busy, or the budget ran out;
    /// nothing was changed.
    Skipped,
    /// `Idle` mode: the copy was interrupted at its budget. Completion is not established
    /// and main-file pages may have been written; `nBackfill` is not published, so no
    /// durable progress is claimed.
    Interrupted,
    /// A PASSIVE checkpoint ran. `shrink_commit_landed` says a sequence-bump maintenance
    /// commit followed it; it does not claim the file shrank.
    Checkpointed {
        complete: bool,
        shrink_commit_landed: bool,
    },
}

/// Length of `-wal`; a missing one is `WalMissing`, as everywhere in the crate.
pub(crate) fn wal_len(root: &Path) -> Result<u64> {
    match fs::symlink_metadata(root.join("store.sqlite3-wal")) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::WalMissing),
        Err(error) => Err(error.into()),
    }
}

/// Lock, connection and SQLite busy results, an expired budget and a fenced store:
/// maintenance skips. A `CommitLost` found by the maintenance commit is reported.
pub(crate) fn skippable(error: &Error) -> bool {
    match error {
        Error::Busy | Error::DeadlineExceeded | Error::Fenced => true,
        Error::Sqlite(rusqlite::Error::SqliteFailure(failure, _)) => matches!(
            failure.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        ),
        _ => false,
    }
}

impl Store {
    pub fn writer_options(&self) -> &WriterOptions {
        &self.options
    }

    /// Open-time PASSIVE checkpoint, under the `io.lock` the caller holds. A busy
    /// checkpoint lock is ignored; PASSIVE never truncates or removes `-wal`.
    pub(crate) fn passive_at_open(
        connection: &rusqlite::Connection,
        deadline: Instant,
    ) -> Result<()> {
        match checkpoint(connection, false, deadline) {
            Err(error) if skippable(&error) => Ok(()),
            other => other.map(drop),
        }
    }

    /// Under `AfterCommit`, runs after a commit's `io.lock` is released (`Store::write` calls
    /// it with the earlier of its own deadline and `MAINTENANCE_BUDGET`). Under `Owner`: reads
    /// the live log size with a NOOP checkpoint; past `PASSIVE_AFTER` it re-takes `io.lock`
    /// within `deadline` and runs PASSIVE; when that checkpoint completed with the `-wal`
    /// file still over `JOURNAL_SIZE_LIMIT` (a reader had pinned it), one maintenance
    /// commit follows. That commit only advances `commit_sequence`, but it writes a frame,
    /// which restarts the log, and SQLite cuts the file to the limit at that write. Busy,
    /// an expired budget or a fence of either state (checked under `io.lock` before
    /// PASSIVE) is a skip, never an error. `deadline` bounds admission, `io.lock`
    /// and busy waits, not the PASSIVE copy itself. Under `Idle` this is the holder's tick
    /// (`wal_idle.rs`): the copy is bounded by `passive_budget`.
    pub fn maintain_wal(&self, deadline: Instant) -> Result<WalMaintenance> {
        self.maintain_wal_with(deadline, self.options.passive_budget)
    }

    /// As `maintain_wal`, with `copy_budget` as the `Idle` copy budget (capped at
    /// `PASSIVE_COPY_BUDGET`). Under `Idle`, `deadline` bounds admission and the `io.lock`
    /// wait only; the copy has its own budget, counted from the moment `io.lock` is held.
    pub fn maintain_wal_with(
        &self,
        deadline: Instant,
        copy_budget: Duration,
    ) -> Result<WalMaintenance> {
        if self.options.checkpoint != CheckpointPolicy::Owner {
            return Ok(WalMaintenance::Idle);
        }
        if self.options.maintenance == Maintenance::Idle {
            return self.maintain_idle(
                Origin::Tick,
                deadline,
                copy_budget.min(PASSIVE_COPY_BUDGET),
            );
        }
        self.maintain_after_commit_mode(deadline)
    }

    fn maintain_after_commit_mode(&self, deadline: Instant) -> Result<WalMaintenance> {
        let done = match self.passive_if_due(deadline) {
            Ok(Some(done)) => done,
            Ok(None) => return Ok(WalMaintenance::Idle),
            Err(error) if skippable(&error) => return Ok(WalMaintenance::Skipped),
            Err(error) => return Err(error),
        };
        let pinned = done.complete && wal_len(self.binding.root())? > JOURNAL_SIZE_LIMIT;
        let landed = pinned
            && match self.commit(deadline, Publish::Nothing, None, |_| Ok(())) {
                Ok(()) => true,
                Err(error) if skippable(&error) => false,
                Err(error) => return Err(error),
            };
        Ok(WalMaintenance::Checkpointed {
            complete: done.complete,
            shrink_commit_landed: landed,
        })
    }

    /// `None` while the log is at most `PASSIVE_AFTER`; otherwise one PASSIVE under `io.lock`.
    fn passive_if_due(&self, deadline: Instant) -> Result<Option<crate::Checkpoint>> {
        let connection = self.admit(deadline)?;
        if LogState::read(&connection)?.live_bytes() <= PASSIVE_AFTER {
            return Ok(None);
        }
        let _io = self.binding.io(deadline)?;
        // A fenced store of either state is not checkpointed: its WAL stays as it was
        // seen. A `commit_lost` fence is reported as `Fenced` so that it is a skip too.
        match crate::fence::check(self.binding.root(), &self.identity.generation) {
            Err(Error::CommitLost) => return Err(Error::Fenced),
            other => other?,
        }
        connection::check_deadline(deadline)?;
        let result = checkpoint(&connection, false, deadline)?;
        self.binding.check()?;
        Ok(Some(result))
    }

    /// Best effort: any maintenance result, including an error, leaves the commit's
    /// result unchanged. `waited` says the commit had to wait for `io.lock`.
    pub(crate) fn maintain_after_commit(&self, deadline: Instant, waited: bool) {
        if self.options.checkpoint != CheckpointPolicy::Owner {
            return;
        }
        match self.options.maintenance {
            Maintenance::AfterCommit => {
                let budget = deadline.min(Instant::now() + MAINTENANCE_BUDGET);
                let _ = self.maintain_wal(budget);
            }
            Maintenance::Idle => self.maintain_fallback(deadline, waited),
        }
    }
}
