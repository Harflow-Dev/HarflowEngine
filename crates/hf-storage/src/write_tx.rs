//! The single commit path of every store write: admission, `io.lock`, an immediate
//! transaction, the caller's action, the dataset publication it asks for, the
//! `commit_sequence` increment, and the closing checks.
use crate::{Error, Result, Store, connection};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::time::Instant;

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
    pub(crate) fn write<T>(
        &self,
        deadline: Instant,
        publish: Publish,
        stage: Option<Stage>,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io(deadline)?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = action(&transaction)?;
        publish.apply(&transaction)?;
        advance_sequence(&transaction)?;
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        fire(stage, false);
        connection::check_deadline(deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        fire(stage, true);
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        Ok(value)
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

fn advance_sequence(transaction: &Transaction<'_>) -> Result<()> {
    let changed = transaction.execute(
        "UPDATE commit_sequence SET value=value+1 WHERE singleton=1",
        [],
    )?;
    if changed != 1 {
        return Err(Error::Incompatible);
    }
    Ok(())
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
