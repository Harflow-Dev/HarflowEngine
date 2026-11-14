//! The single commit path of every store write: admission, `io.lock`, the fence and
//! watermark checks, an immediate transaction, the caller's action, the dataset
//! publication it asks for, the `commit_sequence` increment, the closing checks, and the
//! watermark publication of the new sequence.
use crate::{
    Error, Result, Store, connection,
    watermark::{self, Watermark},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{sync::atomic::Ordering, time::Instant};

pub(crate) const SEQUENCE_SCHEMA: &str = "CREATE TABLE commit_sequence(singleton INTEGER PRIMARY KEY CHECK(singleton=1),value INTEGER NOT NULL); INSERT INTO commit_sequence VALUES(1,0);";

/// What a write publishes to readers of the task list, inside its own transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Publish {
    /// Rotate the dataset (paging) generation and bump the watch revision.
    GenerationAndRevision,
    /// Bump the watch revision only; the action owns any generation change.
    Revision,
    /// Rotate the dataset (paging) generation only.
    Generation,
    /// Publish nothing.
    Nothing,
}

/// Names of the test-only stage hooks fired around the commit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stage {
    pub before: &'static str,
    pub after: &'static str,
}
impl Stage {
    pub(crate) const TASK_CATALOG: Self = Self {
        before: "before_task_catalog_commit",
        after: "after_task_catalog_commit",
    };
    pub(crate) const OWNERSHIP: Self = Self {
        before: "ownership_before_commit",
        after: "ownership_after_commit",
    };
    pub(crate) const CONVERSATION: Self = Self {
        before: "conversation_before_commit",
        after: "conversation_after_commit",
    };
    pub(crate) const CAPTURE: Self = Self {
        before: "capture_before_commit",
        after: "capture_after_commit",
    };
    pub(crate) const CAPTURE_TERMINAL: Self = Self {
        before: "capture_before_terminal",
        after: "capture_after_terminal",
    };
}

impl Store {
    /// One commit, then WAL maintenance once its `io.lock` is released. The maintenance
    /// is best effort under the Owner policy and never changes this write's result.
    pub(crate) fn write<T>(
        &self,
        deadline: Instant,
        publish: Publish,
        stage: Option<Stage>,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let value = self.commit(deadline, publish, stage, action)?;
        self.maintain_after_commit(deadline);
        Ok(value)
    }
    /// As `write`, but an action returning `None` rolls back: nothing is published, the
    /// sequence does not advance and no maintenance runs.
    pub(crate) fn write_unless<T>(
        &self,
        deadline: Instant,
        publish: Publish,
        stage: Option<Stage>,
        action: impl FnOnce(&Transaction<'_>) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let value = self.commit_unless(deadline, publish, stage, action)?;
        if value.is_some() {
            self.maintain_after_commit(deadline);
        }
        Ok(value)
    }
    /// The commit itself, without maintenance.
    pub(crate) fn commit<T>(
        &self,
        deadline: Instant,
        publish: Publish,
        stage: Option<Stage>,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        // The wrapped action always returns `Some`, so `commit_unless` returns `None` only
        // for an action that asked to roll back, which this one never does.
        self.commit_unless(deadline, publish, stage, |tx| action(tx).map(Some))?
            .ok_or(Error::Incompatible)
    }
    fn commit_unless<T>(
        &self,
        deadline: Instant,
        publish: Publish,
        stage: Option<Stage>,
        action: impl FnOnce(&Transaction<'_>) -> Result<Option<T>>,
    ) -> Result<Option<T>> {
        let mut connection = self.admit(deadline)?;
        // A handle whose watermark write or flush failed takes no further write: a flush
        // retried after a failure can report success without the data being durable.
        if self.poisoned.load(Ordering::Acquire) {
            return Err(Error::WatermarkUnrecorded);
        }
        let _io = self.binding.io(deadline)?;
        // A fenced store takes no further commit; the refusal is final, not retried.
        crate::fence::check(self.binding.root(), &self.identity.generation)?;
        let mark = self.open_watermark()?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        watermark::not_ahead(mark.seq(), &transaction)?;
        let Some(value) = action(&transaction)? else {
            return Ok(None);
        };
        publish.apply(&transaction)?;
        let sequence = advance_sequence(&transaction)?;
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        fire(stage, false);
        connection::check_deadline(deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        fire(stage, true);
        // The database is now one ahead of the file; reads and a reopen stay valid.
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        self.record_watermark(&mark, sequence)?;
        Ok(Some(value))
    }
    /// Opens and decodes the watermark under `io.lock`, then flushes it as it stands
    /// before anything overwrites a slot. A failed flush poisons the handle.
    fn open_watermark(&self) -> Result<Watermark> {
        let mark = Watermark::open(self.binding.root(), &self.identity.generation)?;
        if mark.settle(self.options.flush.as_ref()).is_err() {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::WatermarkUnrecorded);
        }
        Ok(mark)
    }
    /// Publishes `sequence` after its commit. A failed write or flush leaves the commit
    /// visible without an established durable acknowledgement: `CommitOutcomeUnknown`,
    /// and the handle is poisoned.
    fn record_watermark(&self, mark: &Watermark, sequence: u64) -> Result<()> {
        let generation = &self.identity.generation;
        if mark
            .record(generation, sequence, self.options.flush.as_ref())
            .is_err()
        {
            self.poisoned.store(true, Ordering::Release);
            return Err(Error::CommitOutcomeUnknown);
        }
        Ok(())
    }
    /// Alias for writers that publish nothing and fire no stage hook.
    pub(crate) fn fixture_write<T>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        self.write(deadline, Publish::Nothing, None, action)
    }
}

impl Publish {
    fn apply(self, transaction: &Transaction<'_>) -> Result<()> {
        match self {
            Self::GenerationAndRevision => {
                rotate_generation(transaction)?;
                crate::watch_meta::publish(transaction)
            }
            Self::Revision => crate::watch_meta::publish(transaction),
            Self::Generation => rotate_generation(transaction),
            Self::Nothing => Ok(()),
        }
    }
}

fn rotate_generation(transaction: &Transaction<'_>) -> Result<()> {
    crate::task_read::dataset_optional(transaction)?.ok_or(Error::TaskReadUnavailable)?;
    transaction.execute(
        "UPDATE task_dataset SET generation=lower(hex(randomblob(16))) WHERE singleton=1",
        [],
    )?;
    Ok(())
}

/// Increments the sequence and returns the new value, the number of this commit.
fn advance_sequence(transaction: &Transaction<'_>) -> Result<u64> {
    let value: i64 = transaction
        .query_row(
            "UPDATE commit_sequence SET value=value+1 WHERE singleton=1 RETURNING value",
            [],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Error::Incompatible,
            other => other.into(),
        })?;
    u64::try_from(value).map_err(|_| Error::Incompatible)
}

/// A store without the sequence row (an older local schema 7 root) is not a writable store.
pub(crate) fn require_sequence(connection: &Connection) -> Result<()> {
    let tables: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='commit_sequence'",
        [],
        |row| row.get(0),
    )?;
    if tables != 1 {
        return Err(Error::Incompatible);
    }
    let rows: i64 = connection.query_row(
        "SELECT count(*) FROM commit_sequence WHERE singleton=1 AND typeof(value)='integer'",
        [],
        |row| row.get(0),
    )?;
    if rows != 1 {
        return Err(Error::Incompatible);
    }
    Ok(())
}

fn fire(stage: Option<Stage>, after: bool) {
    let name = stage.map(|stage| if after { stage.after } else { stage.before });
    #[cfg(all(test, unix))]
    if let Some(name) = name {
        crate::tests::stage(name);
    }
    #[cfg(not(all(test, unix)))]
    let _ = name;
}
