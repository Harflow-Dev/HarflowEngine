//! Writer-side WAL ownership: checkpoint policy and post-commit maintenance.
//!
//! `Owner` bounds the `-wal` file: `journal_size_limit` 16 MiB, an open-time PASSIVE
//! checkpoint, and a PASSIVE checkpoint after a commit once the live log passes 8 MiB. It keeps
//! both SQLite checkpoint suppressions (`wal_autocheckpoint=0`, no checkpoint on close),
//! never uses TRUNCATE (only `recover` does) and never RESTART, so `-wal` always survives.
//! `Suppressed` is the behaviour of the fixture seed, the host fixture modes and the
//! watch seam: nothing checkpoints, so seeded roots keep their nonempty `-wal`.
use crate::{Error, Result, Store, checkpoint::checkpoint, connection, write_tx::Publish};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointPolicy {
    /// This handle checkpoints and bounds `-wal`.
    Owner,
    /// Today's behaviour: no checkpoint of any kind.
    Suppressed,
}

/// Chosen by the caller of `Store::open_with`; `Store::open` is `WriterOptions::suppressed()`.
/// `flush` makes the commit watermark durable: it runs twice per commit, before the slot
/// write and after it.
#[derive(Debug, Clone)]
pub struct WriterOptions {
    pub checkpoint: CheckpointPolicy,
    pub flush: Arc<dyn FlushPort>,
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
        }
    }
    pub fn owner() -> Self {
        Self {
            checkpoint: CheckpointPolicy::Owner,
            flush: default_flush(),
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
/// maintenance skips.
fn skippable(error: &Error) -> bool {
    match error {
        Error::Busy | Error::DeadlineExceeded | Error::Fenced => true,
        Error::Sqlite(rusqlite::Error::SqliteFailure(failure, _)) => matches!(
            failure.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        ),
        _ => false,
    }
}

/// Bytes of the live log (frames since the last restart), read with a NOOP checkpoint:
/// no checkpoint lock, no copy, no file change, though reading the WAL-index header can
/// run WAL-index recovery under its own locks. A busy result or an unknown log size
/// (`busy` = 1 or a negative frame count) is `Busy`, so maintenance skips. The `-wal`
/// file length is not the log size: after a restart the file keeps its length while the
/// log refills from the start.
fn log_bytes(connection: &rusqlite::Connection) -> Result<u64> {
    let (busy, frames): (i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(NOOP)", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    if busy != 0 || frames < 0 {
        return Err(Error::Busy);
    }
    let page: i64 = connection.pragma_query_value(None, "page_size", |row| row.get(0))?;
    let frames = u64::try_from(frames).map_err(|_| Error::Incompatible)?;
    let page = u64::try_from(page).map_err(|_| Error::Incompatible)?;
    Ok(32 + frames * (page + 24))
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

    /// Runs after a commit's `io.lock` is released (`Store::write` calls it with the
    /// earlier of its own deadline and `MAINTENANCE_BUDGET`). Under `Owner`: reads the
    /// live log size with a NOOP checkpoint; past `PASSIVE_AFTER` it re-takes `io.lock`
    /// within `deadline` and runs PASSIVE; when that checkpoint completed with the `-wal`
    /// file still over `JOURNAL_SIZE_LIMIT` (a reader had pinned it), one maintenance
    /// commit follows. That commit only advances `commit_sequence`, but it writes a frame,
    /// which restarts the log, and SQLite cuts the file to the limit at that write. Busy,
    /// an expired budget or a fence (checked under `io.lock` before PASSIVE) is a skip,
    /// never an error. `deadline` bounds admission, `io.lock`
    /// and busy waits, not the PASSIVE copy itself.
    pub fn maintain_wal(&self, deadline: Instant) -> Result<WalMaintenance> {
        if self.options.checkpoint != CheckpointPolicy::Owner {
            return Ok(WalMaintenance::Idle);
        }
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
        if log_bytes(&connection)? <= PASSIVE_AFTER {
            return Ok(None);
        }
        let _io = self.binding.io(deadline)?;
        // A fenced store is not checkpointed: its WAL stays as the verifier saw it.
        crate::fence::check(self.binding.root(), &self.identity.generation)?;
        connection::check_deadline(deadline)?;
        let result = checkpoint(&connection, false, deadline)?;
        self.binding.check()?;
        Ok(Some(result))
    }

    /// Best effort: any maintenance result, including an error, leaves the commit's
    /// result unchanged.
    pub(crate) fn maintain_after_commit(&self, deadline: Instant) {
        if self.options.checkpoint == CheckpointPolicy::Owner {
            let budget = deadline.min(Instant::now() + MAINTENANCE_BUDGET);
            let _ = self.maintain_wal(budget);
        }
    }
}
