//! Idle WAL maintenance of the task writer: cadence, backoff and the failure classification.
//!
//! The writer loop calls `Writer::maintain_idle` on every pending receive. Calls are paced
//! here: at most one per `BASE` interval, doubling up to `CAP` after an interrupted,
//! skipped or incomplete outcome and resetting after a complete one. The attempt timestamp,
//! the interruption count and the long backoff live in the store (`hf_storage`); this module
//! keeps no copy of them.
use super::Error;
use hf_storage::{MAINTENANCE_BUDGET, WalMaintenance};
use std::time::{Duration, Instant};

/// Interval between idle calls while maintenance succeeds or has nothing to do.
const BASE: Duration = Duration::from_millis(250);
/// Largest interval after repeated interrupted or skipped outcomes.
const CAP: Duration = Duration::from_secs(4);

/// When the next idle call may run.
#[derive(Debug)]
pub(super) struct Cadence {
    interval: Duration,
    next: Option<Instant>,
}

impl Cadence {
    /// The first call is due at once: the writer's open performs no checkpoint.
    pub(super) fn new() -> Self {
        Self {
            interval: BASE,
            next: None,
        }
    }

    pub(super) fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    /// Sets the next call after `outcome` finished at `now`. A failure that does not poison
    /// counts as a skip.
    pub(super) fn settle(&mut self, outcome: Option<&WalMaintenance>, now: Instant) {
        self.interval = match outcome {
            Some(WalMaintenance::Idle | WalMaintenance::Checkpointed { complete: true, .. }) => {
                BASE
            }
            _ => (self.interval * 2).min(CAP),
        };
        self.next = Some(now + self.interval);
    }
}

/// Whether a maintenance error leaves the writer's own state uncertain: a shrink commit
/// whose effect is unknown, a watermark that did not record, a lost commit or a replaced
/// or invalid root. Every other failure (read-only NOOP I/O, a missing `-wal`, a failed
/// transient open) changed nothing durable and is recorded as a skip.
pub(super) fn poisons(error: &hf_storage::Error) -> bool {
    use hf_storage::Error as E;
    matches!(
        error,
        E::CommitOutcomeUnknown
            | E::WatermarkUnrecorded
            | E::CommitLost
            | E::Replaced
            | E::InvalidRoot
    )
}

/// One idle step: `call` when due, then pace by its outcome. A poisoning error is
/// returned as the loop's error value; any other error backs off.
pub(super) fn step(
    cadence: &mut Cadence,
    now: Instant,
    call: impl FnOnce() -> hf_storage::Result<WalMaintenance>,
) -> Result<(), Error> {
    if !cadence.due(now) {
        return Ok(());
    }
    match call() {
        Ok(outcome) => cadence.settle(Some(&outcome), Instant::now()),
        Err(error) if poisons(&error) => return Err(Error::Storage(error)),
        Err(_) => cadence.settle(None, Instant::now()),
    }
    Ok(())
}

/// The final call: a short copy budget, outside the cadence. The terminal is already
/// committed, so the outcome is never an error for the caller; a poisoning failure is
/// reported as `true` so the caller can record it.
pub(super) fn finish(store: &hf_storage::Store) -> bool {
    let outcome = store.maintain_wal_with(Instant::now() + MAINTENANCE_BUDGET, MAINTENANCE_BUDGET);
    matches!(outcome, Err(ref error) if poisons(error))
}

#[cfg(test)]
#[path = "idle_maintenance_tests.rs"]
mod tests;
