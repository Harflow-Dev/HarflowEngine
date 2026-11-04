use crate::{Error, Result, Store, connection};
use hf_application::task_read::TaskRow;
use hf_domain::task_read::{Coverage, TaskId};
use rusqlite::TransactionBehavior;
use std::time::Instant;
pub const TASK_SCHEMA: &str = "CREATE TABLE task_dataset(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,generation TEXT NOT NULL CHECK(typeof(generation)='text' AND length(generation)=32),discovery INTEGER NOT NULL CHECK(discovery IN(0,1)),ownership INTEGER NOT NULL CHECK(ownership IN(0,1)),source INTEGER NOT NULL CHECK(source IN(0,1))); CREATE TABLE task_headers(id TEXT PRIMARY KEY CHECK(typeof(id)='text' AND length(id) BETWEEN 1 AND 64),stamp INTEGER NOT NULL,session TEXT,active INTEGER CHECK(active IN(0,1)),recorded_status TEXT,exited INTEGER CHECK(exited IN(0,1)),drained INTEGER CHECK(drained IN(0,1)),owned INTEGER CHECK(owned IN(0,1)),header BLOB NOT NULL,status BLOB); CREATE INDEX task_order ON task_headers(stamp DESC,id DESC); CREATE INDEX task_session ON task_headers(session,stamp DESC,id DESC); CREATE INDEX task_active ON task_headers(active,stamp DESC,id DESC); CREATE TABLE task_pending(id TEXT PRIMARY KEY CHECK(typeof(id)='text' AND length(id) BETWEEN 1 AND 64));";
pub struct TaskFixture {
    pub rows: Vec<TaskRow>,
    pub pending: Vec<TaskId>,
    pub coverage: Coverage,
}
impl Store {
    /// Explicit fixture replacement only; never creates or upgrades read tables.
    pub fn replace_task_fixture(&self, fixture: &TaskFixture, deadline: Instant) -> Result<()> {
        validate(self, fixture, deadline)?;
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io()?;
        connection::timeout(&connection, deadline)?;
        super::task_read::require_tables(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::task_read::dataset_optional(&transaction)?;
        connection::timeout(&transaction, deadline)?;
        transaction.execute_batch(
            "DELETE FROM task_headers; DELETE FROM task_pending; DELETE FROM task_dataset;",
        )?;
        transaction.execute(
            "INSERT INTO task_dataset VALUES(1,1,lower(hex(randomblob(16))),?1,?2,?3)",
            rusqlite::params![
                fixture.coverage.discovery_complete,
                fixture.coverage.ownership_complete,
                fixture.coverage.source_complete
            ],
        )?;
        for row in &fixture.rows {
            connection::timeout(&transaction, deadline)?;
            transaction.execute(
                "INSERT INTO task_headers VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    row.id.as_str(),
                    row.stamp,
                    row.session,
                    row.observation.active,
                    row.observation.recorded_status,
                    row.observation.exited,
                    row.observation.drained,
                    row.observation.owned,
                    row.header,
                    row.status
                ],
            )?;
        }
        for id in &fixture.pending {
            connection::timeout(&transaction, deadline)?;
            transaction.execute("INSERT INTO task_pending VALUES(?1)", [id.as_str()])?;
        }
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)
    }
}
fn validate(store: &Store, fixture: &TaskFixture, deadline: Instant) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    store.check_bounds(fixture.rows.len().saturating_add(fixture.pending.len()), 0)?;
    for row in &fixture.rows {
        connection::check_deadline(deadline)?;
        if !ids.insert(row.id.clone())
            || row.header.len() > 2048
            || row.status.as_ref().is_some_and(|s| s.len() > 2048)
            || row
                .session
                .as_ref()
                .is_some_and(|s| s.len() > 1024 || s.chars().take(257).count() > 256)
            || row
                .observation
                .recorded_status
                .as_ref()
                .is_some_and(|s| s.len() > 256)
        {
            return Err(Error::LimitExceeded);
        }
        bytes = bytes
            .checked_add(super::task_read::row_bytes(row))
            .ok_or(Error::LimitExceeded)?;
    }
    for id in &fixture.pending {
        connection::check_deadline(deadline)?;
        if !ids.insert(id.clone()) {
            return Err(Error::Incompatible);
        }
        bytes = bytes
            .checked_add(id.as_str().len())
            .ok_or(Error::LimitExceeded)?;
    }
    store.check_bounds(ids.len(), bytes)
}
