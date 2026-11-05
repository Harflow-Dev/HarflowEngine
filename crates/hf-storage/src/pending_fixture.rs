use crate::{Error, Result, Store, connection};
use hf_domain::task_read::TaskId;
use rusqlite::TransactionBehavior;
use std::time::Instant;
impl Store {
    /// Pending-only fixture update; invalidates task paging but never task-list metadata.
    pub fn replace_pending_fixture(&self, pending: &[TaskId], deadline: Instant) -> Result<()> {
        self.check_bounds(pending.len(), 0)?;
        let mut bytes = 0usize;
        let mut unique = std::collections::BTreeSet::new();
        for id in pending {
            connection::check_deadline(deadline)?;
            bytes = bytes
                .checked_add(id.as_str().len())
                .ok_or(Error::LimitExceeded)?;
            self.check_bounds(pending.len(), bytes)?;
            if !unique.insert(id) {
                return Err(Error::Incompatible);
            }
        }
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io()?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::task_read::dataset_optional(&transaction)?.ok_or(Error::TaskReadUnavailable)?;
        for id in pending {
            connection::timeout(&transaction, deadline)?;
            let collision: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_headers WHERE id=?1)",
                [id.as_str()],
                |row| row.get(0),
            )?;
            if collision {
                return Err(Error::Incompatible);
            }
        }
        transaction.execute("DELETE FROM task_pending", [])?;
        for id in pending {
            connection::timeout(&transaction, deadline)?;
            transaction.execute("INSERT INTO task_pending VALUES(?1)", [id.as_str()])?;
        }
        transaction.execute(
            "UPDATE task_dataset SET generation=lower(hex(randomblob(16))) WHERE singleton=1",
            [],
        )?;
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
