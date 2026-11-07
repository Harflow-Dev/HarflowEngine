//! One transaction shape for every fixture writer: admission, `io.lock`, an immediate
//! transaction, and the same closing checks as `replace_task_fixture`.
use crate::{Error, Result, Store, connection};
use rusqlite::{Transaction, TransactionBehavior};
use std::time::Instant;
impl Store {
    pub(crate) fn fixture_write<T>(
        &self,
        deadline: Instant,
        action: impl FnOnce(&Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.admit(deadline)?;
        let _io = self.binding.io(deadline)?;
        connection::timeout(&connection, deadline)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = action(&transaction)?;
        self.binding.check()?;
        connection::timeout(&transaction, deadline)?;
        connection::check_deadline(deadline)?;
        transaction
            .commit()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)?;
        Ok(value)
    }
}
/// A task write changes the dataset generation and publishes the watch revision.
pub(crate) fn task_changed(transaction: &Transaction<'_>) -> Result<()> {
    crate::task_read::dataset_optional(transaction)?.ok_or(Error::TaskReadUnavailable)?;
    transaction.execute(
        "UPDATE task_dataset SET generation=lower(hex(randomblob(16))) WHERE singleton=1",
        [],
    )?;
    crate::watch_meta::publish(transaction)
}
