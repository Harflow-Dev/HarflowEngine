//! `Maintenance::Idle`: bounded WAL maintenance driven by the task writer's idle loop.
//!
//! A tick (`Store::maintain_wal`) reads the log with a NOOP checkpoint. A PASSIVE is due when
//! the uncheckpointed backlog passes `PASSIVE_AFTER`; a shrink commit is due when `-wal` is
//! over `JOURNAL_SIZE_LIMIT` and nothing is left to copy. The PASSIVE runs on a transient
//! connection under `io.lock`, interrupted by the scan watchdog at `passive_budget`, which
//! starts counting once `io.lock` is held. An interrupted PASSIVE publishes no progress.
//! The commit path falls back to the same routine only above `HARD_LIVE_LOG`.
//!
//! Cooperative bound: the deadline cannot cut the WAL fsync before the copy, connection
//! setup, the database sync after a completed copy, or the shrink commit.
use crate::{
    Checkpoint, Error, Result, Store, WalMaintenance, connection, fence, scan,
    wal_owner::{
        HARD_LIVE_LOG, JOURNAL_SIZE_LIMIT, MAINTENANCE_BUDGET, PASSIVE_AFTER, PASSIVE_COPY_BUDGET,
        skippable, wal_len,
    },
    write_tx::Publish,
};
use rusqlite::Connection;
use std::{
    cell::Cell,
    sync::{MutexGuard, PoisonError},
    time::{Duration, Instant},
};

/// A maintenance attempt that started within this window of a commit's acknowledgement
/// keeps the commit path from copying again.
pub(crate) const ACK_WINDOW: Duration = Duration::from_secs(5);
/// Pause after `INTERRUPT_LIMIT` consecutive interrupted copies.
pub(crate) const BACKOFF: Duration = Duration::from_secs(60);
pub(crate) const INTERRUPT_LIMIT: u32 = 3;
/// Bytes of one WAL frame beyond the page: the frame header.
const FRAME_HEADER: u64 = 24;
/// Bytes of the WAL file header.
const WAL_HEADER: u64 = 32;

/// Who asks: the holder's idle tick, or the commit path's fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Tick,
    Fallback,
}

/// How a PASSIVE that took `io.lock` ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// Interrupted at the handle's full copy budget: counts toward the long backoff.
    Interrupted,
    /// Interrupted at a budget shorter than the handle's (the fallback or the final call):
    /// says nothing about the envelope, so it behaves like a skip.
    Cut,
    Complete,
    Incomplete,
}

/// Idle maintenance state of one `Store`: the commit path and the idle tick share it.
#[derive(Debug, Default)]
pub(crate) struct State {
    last_attempt: Option<Instant>,
    interrupts: u32,
    backoff_until: Option<Instant>,
    exceeded: bool,
}

impl State {
    /// Whether `origin` may start an attempt at `now`: never inside a long backoff, and
    /// the fallback never within `ACK_WINDOW` of the last attempt.
    pub(crate) fn allows(&self, origin: Origin, now: Instant) -> bool {
        if self.backoff_until.is_some_and(|until| now < until) {
            return false;
        }
        origin == Origin::Tick
            || self
                .last_attempt
                .is_none_or(|at| now.saturating_duration_since(at) >= ACK_WINDOW)
    }

    /// A PASSIVE took `io.lock` at `now`.
    pub(crate) fn begin(&mut self, now: Instant) {
        self.last_attempt = Some(now);
    }

    pub(crate) fn record(&mut self, attempt: Attempt, now: Instant) {
        match attempt {
            Attempt::Interrupted => {
                self.interrupts += 1;
                if self.interrupts >= INTERRUPT_LIMIT {
                    self.backoff_until = Some(now + BACKOFF);
                    self.interrupts = 0;
                    self.exceeded = true;
                }
            }
            Attempt::Cut => {}
            Attempt::Incomplete => self.interrupts = 0,
            Attempt::Complete => {
                *self = Self {
                    last_attempt: self.last_attempt,
                    ..Self::default()
                };
            }
        }
    }

    /// Test seam: pretend the last attempt started `ago` before now (or never).
    #[cfg(test)]
    pub(crate) fn set_last_attempt(&mut self, ago: Option<Duration>) {
        self.last_attempt = ago.map(|ago| Instant::now() - ago);
    }

    /// Test seam: enter or leave the long backoff.
    #[cfg(test)]
    pub(crate) fn set_backoff(&mut self, until: Option<Instant>) {
        self.backoff_until = until;
    }

    /// Copies were interrupted often enough to enter the long backoff, and no complete
    /// checkpoint has happened since.
    pub(crate) fn exceeded(&self) -> bool {
        self.exceeded
    }
}

/// The log as a NOOP checkpoint reports it: no checkpoint lock, no copy, no file change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LogState {
    frames: u64,
    done: u64,
    frame_bytes: u64,
}

impl LogState {
    /// A busy result or an unknown size (`busy` = 1 or a negative count) is `Busy`, so
    /// maintenance skips. The `-wal` file length is not the log size: after a restart the
    /// file keeps its length while the log refills from the start.
    pub(crate) fn read(connection: &Connection) -> Result<Self> {
        let (busy, frames, done): (i64, i64, i64) =
            connection.query_row("PRAGMA wal_checkpoint(NOOP)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        if busy != 0 || frames < 0 || done < 0 {
            return Err(Error::Busy);
        }
        let page: i64 = connection.pragma_query_value(None, "page_size", |row| row.get(0))?;
        let page = u64::try_from(page).map_err(|_| Error::Incompatible)?;
        Ok(Self {
            frames: u64::try_from(frames).map_err(|_| Error::Incompatible)?,
            done: u64::try_from(done).map_err(|_| Error::Incompatible)?,
            frame_bytes: page + FRAME_HEADER,
        })
    }

    /// Bytes of the live log (frames since the last restart).
    pub(crate) fn live_bytes(&self) -> u64 {
        WAL_HEADER + self.frames * self.frame_bytes
    }

    /// Bytes of the log not yet copied into the main file.
    pub(crate) fn backlog_bytes(&self) -> u64 {
        self.frames.saturating_sub(self.done) * self.frame_bytes
    }

    fn drained(&self) -> bool {
        self.frames <= self.done
    }
}

/// What the copy did.
enum Passive {
    Done(Checkpoint),
    Interrupted,
}

impl Store {
    fn idle_state(&self) -> MutexGuard<'_, State> {
        self.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether copies were interrupted often enough to enter the long backoff, with no
    /// complete checkpoint since: the supported envelope was exceeded.
    pub fn wal_envelope_exceeded(&self) -> bool {
        self.idle_state().exceeded()
    }

    /// One `Idle` attempt. Lock, connection and SQLite busy results, an expired admission
    /// budget and a fence are skips; any other error is returned.
    pub(crate) fn maintain_idle(
        &self,
        origin: Origin,
        deadline: Instant,
        copy_budget: Duration,
    ) -> Result<WalMaintenance> {
        match self.idle_attempt(origin, deadline, copy_budget) {
            Err(error) if skippable(&error) => Ok(WalMaintenance::Skipped),
            other => other,
        }
    }

    fn idle_attempt(
        &self,
        origin: Origin,
        deadline: Instant,
        budget: Duration,
    ) -> Result<WalMaintenance> {
        if !self.idle_state().allows(origin, Instant::now()) {
            return Ok(WalMaintenance::Skipped);
        }
        let Some(passive) = self.idle_passive(origin, deadline, budget)? else {
            return Ok(WalMaintenance::Idle);
        };
        let done = match passive {
            Passive::Interrupted => return Ok(WalMaintenance::Interrupted),
            Passive::Done(done) => done,
        };
        let oversized = done.complete && wal_len(self.binding.root())? > JOURNAL_SIZE_LIMIT;
        let landed = oversized && self.shrink_commit()?;
        Ok(WalMaintenance::Checkpointed {
            complete: done.complete,
            shrink_commit_landed: landed,
        })
    }

    /// `None` when nothing is due. The source connection stays held for the whole attempt,
    /// so no other user of this handle commits meanwhile.
    fn idle_passive(
        &self,
        origin: Origin,
        deadline: Instant,
        budget: Duration,
    ) -> Result<Option<Passive>> {
        let source = self.admit(deadline)?;
        if !self.idle_due(origin, &source)? {
            return Ok(None);
        }
        // Opened before `io.lock`, so setup time is not spent holding it.
        let transient = connection::open_transient(&self.binding, &self.identity, deadline)?;
        let (_io, _) = self
            .binding
            .io_polling(deadline, crate::io_wait::IO_POLL, None)?;
        #[cfg(test)]
        hooks::after_io_lock();
        match fence::check(self.binding.root(), &self.identity.generation) {
            Err(Error::CommitLost) => return Err(Error::Fenced),
            other => other?,
        }
        connection::check_deadline(deadline)?;
        let started = Instant::now();
        // An attempt starts when the statement is entered, not when `io.lock` is taken: a
        // deadline that passes before that is a skip and counts for nothing.
        let outcome = passive_bounded(&transient, started + budget, || {
            self.idle_state().begin(Instant::now());
        })?;
        self.binding.check()?;
        let full = budget >= self.options.passive_budget.min(PASSIVE_COPY_BUDGET);
        let attempt = match &outcome {
            Passive::Interrupted if full => Attempt::Interrupted,
            Passive::Interrupted => Attempt::Cut,
            Passive::Done(done) if done.complete => Attempt::Complete,
            Passive::Done(_) => Attempt::Incomplete,
        };
        self.idle_state().record(attempt, Instant::now());
        Ok(Some(outcome))
    }

    /// PASSIVE is due on the backlog (the fallback also needs a live log above
    /// `HARD_LIVE_LOG`). A tick also finds a shrink due: `-wal` over the limit with
    /// nothing left to copy (a skipped shrink, or a Suppressed restart that did not cut
    /// the file); that runs the same routine, whose PASSIVE then has nothing to copy.
    fn idle_due(&self, origin: Origin, source: &Connection) -> Result<bool> {
        let log = LogState::read(source)?;
        let passive = log.backlog_bytes() > PASSIVE_AFTER
            && (origin == Origin::Tick || log.live_bytes() > HARD_LIVE_LOG);
        if passive || origin != Origin::Tick {
            return Ok(passive);
        }
        Ok(log.drained() && wal_len(self.binding.root())? > JOURNAL_SIZE_LIMIT)
    }

    /// One maintenance commit under a fresh `MAINTENANCE_BUDGET`: it only advances
    /// `commit_sequence`, but it writes a frame, which restarts the log and cuts `-wal`
    /// to the limit. A skip is reported as `false` and retried by a later tick.
    fn shrink_commit(&self) -> Result<bool> {
        let deadline = Instant::now() + MAINTENANCE_BUDGET;
        match self.commit(deadline, Publish::Nothing, None, |_| Ok(())) {
            Ok(()) => Ok(true),
            Err(error) if skippable(&error) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// The commit path's fallback: above the hard bound only, never after a commit that
    /// waited for `io.lock`, with a copy budget that leaves room for the acknowledgement.
    pub(crate) fn maintain_fallback(&self, deadline: Instant, waited: bool) {
        if waited {
            return;
        }
        let now = Instant::now();
        let budget = self
            .options
            .passive_budget
            .min(PASSIVE_COPY_BUDGET)
            .min(deadline.saturating_duration_since(now) / 3);
        if budget.is_zero() {
            return;
        }
        let admission = deadline.min(now + MAINTENANCE_BUDGET);
        let _ = self.maintain_idle(Origin::Fallback, admission, budget);
    }
}

/// PASSIVE under the watchdog on a transient connection, which is dropped by the caller
/// and never reused. `entered` runs when the statement is about to start. A deadline that
/// has passed before that (including inside the watchdog's own setup) is
/// `DeadlineExceeded` with `entered` not called: nothing started. An interrupt after the
/// statement started is `Interrupted`; a completion that lands just after the deadline is
/// reported as complete.
fn passive_bounded(
    connection: &Connection,
    deadline: Instant,
    entered: impl FnOnce(),
) -> Result<Passive> {
    let started = Cell::new(false);
    let outcome = scan::bounded_late(connection, deadline, |db| {
        started.set(true);
        entered();
        #[cfg(test)]
        hooks::entered();
        db.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
    });
    match outcome {
        Ok((busy, log_frames, checkpointed_frames)) => Ok(Passive::Done(Checkpoint {
            busy,
            log_frames,
            checkpointed_frames,
            complete: busy == 0 && log_frames == checkpointed_frames,
        })),
        Err(Error::DeadlineExceeded) if started.get() => Ok(Passive::Interrupted),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) fn passive_for_test(
    connection: &Connection,
    deadline: Instant,
    entered: impl FnOnce(),
) -> Result<Option<Checkpoint>> {
    passive_bounded(connection, deadline, entered).map(|passive| match passive {
        Passive::Done(done) => Some(done),
        Passive::Interrupted => None,
    })
}

/// Test-only seams, thread-local so that concurrent tests do not see each other: not
/// reachable from a non-test build.
#[cfg(test)]
pub(crate) mod hooks {
    use std::cell::{Cell, RefCell};

    thread_local! {
        static ENTERED: Cell<usize> = const { Cell::new(0) };
        static AFTER_IO: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    }

    pub(crate) fn entered() {
        ENTERED.with(|count| count.set(count.get() + 1));
    }
    /// How many PASSIVE statements this thread has started.
    pub(crate) fn entries() -> usize {
        ENTERED.with(Cell::get)
    }
    pub(crate) fn after_io_lock() {
        AFTER_IO.with(|hook| {
            if let Some(hook) = hook.borrow().as_ref() {
                hook();
            }
        });
    }
    /// Runs `hook` on this thread right after an idle attempt takes `io.lock`.
    pub(crate) fn set_after_io_lock(hook: Option<Box<dyn Fn()>>) {
        AFTER_IO.with(|slot| *slot.borrow_mut() = hook);
    }
}
