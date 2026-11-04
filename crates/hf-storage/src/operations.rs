use crate::{Error, Mutation, Record, Result, Store, connection};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    sync::{MutexGuard, TryLockError},
    time::Instant,
};
impl Store {
    pub(crate) fn admit(&self, deadline: Instant) -> Result<MutexGuard<'_, Connection>> {
        connection::check_deadline(deadline)?;
        let guard = self.connection.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::Busy,
            TryLockError::Poisoned(_) => Error::Incompatible,
        })?;
        self.binding.check()?;
        if crate::wal::marker(&self.binding)? != self.identity {
            return Err(Error::Replaced);
        }
        if connection::identity(&guard, deadline)? != self.identity {
            return Err(Error::Replaced);
        }
        connection::timeout(&guard, deadline)?;
        Ok(guard)
    }
    pub(crate) fn check_bounds(&self, rows: usize, bytes: usize) -> Result<()> {
        if rows > self.limits.max_rows || bytes > self.limits.max_bytes {
            Err(Error::LimitExceeded)
        } else {
            Ok(())
        }
    }
    pub fn apply_batch(&self, mutations: &[Mutation], deadline: Instant) -> Result<()> {
        connection::check_deadline(deadline)?;
        self.check_bounds(mutations.len(), 0)?;
        let mut bytes = 0usize;
        for mutation in mutations {
            connection::check_deadline(deadline)?;
            let (key, value) = match mutation {
                Mutation::Insert { key, value } | Mutation::Put { key, value } => {
                    (key, value.len())
                }
                Mutation::Delete { key } => (key, 0),
            };
            bytes = bytes
                .checked_add(key.len())
                .and_then(|size| size.checked_add(value))
                .ok_or(Error::LimitExceeded)?;
        }
        self.check_bounds(mutations.len(), bytes)?;
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io()?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for mutation in mutations {
            connection::timeout(&transaction, deadline)?;
            match mutation {
                Mutation::Insert {key,value}=>transaction.execute("INSERT INTO fixture_records(key,value) VALUES(?1,?2)",rusqlite::params![key,value])?,
                Mutation::Put {key,value}=>transaction.execute("INSERT INTO fixture_records(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key,value])?,
                Mutation::Delete {key}=>transaction.execute("DELETE FROM fixture_records WHERE key=?1",[key])?,
            };
        }
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        // A synchronous syscall cannot be preempted at the deadline. No request is
        // queued for later execution; a completed commit is reported truthfully.
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)
    }
    pub fn read_batch(&self, keys: &[String], deadline: Instant) -> Result<Vec<Record>> {
        connection::check_deadline(deadline)?;
        self.check_bounds(keys.len(), 0)?;
        let input_bytes = keys.iter().try_fold(0usize, |sum, key| {
            connection::check_deadline(deadline)?;
            sum.checked_add(key.len()).ok_or(Error::LimitExceeded)
        })?;
        self.check_bounds(keys.len(), input_bytes)?;
        let mut connection = self.admit(deadline)?;
        let transaction = connection.transaction()?;
        let mut records = Vec::new();
        let mut bytes = 0usize;
        for key in keys {
            connection::timeout(&transaction, deadline)?;
            let length: Option<i64> = transaction
                .query_row(
                    "SELECT length(value) FROM fixture_records WHERE key=?1",
                    [key],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(length) = length {
                let length = usize::try_from(length).map_err(|_| Error::Incompatible)?;
                bytes = bytes
                    .checked_add(key.len())
                    .and_then(|size| size.checked_add(length))
                    .ok_or(Error::LimitExceeded)?;
                self.check_bounds(records.len() + 1, bytes)?;
                connection::timeout(&transaction, deadline)?;
                let value = transaction.query_row(
                    "SELECT value FROM fixture_records WHERE key=?1",
                    [key],
                    |row| row.get(0),
                )?;
                records.push(Record {
                    key: key.clone(),
                    value,
                });
            }
        }
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(records)
    }
}
