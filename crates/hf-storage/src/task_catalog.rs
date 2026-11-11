//! Task records and keyset batches for the ctl task reads. Every variable-length value is
//! measured in SQL and checked before it is copied, and bytes are accounted as read.
use crate::{Error, Result, caller_scan::process_record, snapshot::TaskSnapshot, statements};
use hf_domain::{
    ctl::task_rows::{BatchQuery, ChainRoot, TaskFacts, TaskRecord, UnobservedRow},
    task_read::TaskId,
};
use rusqlite::{Row, params_from_iter, types::Value};

const BLOB_LIMIT: i64 = 2048;

pub(crate) fn task_id(text: String) -> Result<TaskId> {
    TaskId::new(text).map_err(|_| Error::MalformedRow)
}

fn optional_id(text: Option<String>) -> Result<Option<TaskId>> {
    text.map(task_id).transpose()
}

/// A value longer than `limit` bytes is refused before it is read.
pub(crate) fn within(length: Option<i64>, limit: i64) -> Result<usize> {
    match length {
        Some(length) if (0..=limit).contains(&length) => {
            usize::try_from(length).map_err(|_| Error::MalformedRow)
        }
        None => Ok(0),
        Some(_) => Err(Error::LimitExceeded),
    }
}

/// The record of the row and the bytes it holds.
fn record(row: &Row<'_>) -> Result<(TaskRecord, usize)> {
    let header_len = within(row.get(21)?, BLOB_LIMIT)?;
    let status_len = within(row.get(22)?, BLOB_LIMIT)?;
    // The text columns have no limit of their own: the query budget bounds the record.
    let text_len = within(row.get(23)?, i64::MAX)?;
    let header: Vec<u8> = row.get(15)?;
    let status_blob: Option<Vec<u8>> = row.get(16)?;
    let process = match row.get::<_, Option<i64>>(17)? {
        Some(pid) => Some(process_record(
            pid,
            row.get(18)?,
            row.get(19)?,
            row.get(20)?,
        )?),
        None => None,
    };
    let facts = TaskFacts {
        id: task_id(row.get(0)?)?,
        backend: row.get(1)?,
        repo_path: row.get(2)?,
        session_id: row.get(3)?,
        status: row.get(4)?,
        exit_code: row.get(5)?,
        started_at: row.get(6)?,
        started_at_us: row.get(7)?,
        finished_at: row.get(8)?,
        finished_at_us: row.get(9)?,
        output_drained: row.get(10)?,
        title: row.get(11)?,
        model: row.get(12)?,
        continuation_of: optional_id(row.get(13)?)?,
        chain_root: ChainRoot::qualify(row.get(14)?),
    };
    let record = TaskRecord {
        facts,
        header,
        status_blob,
        process,
    };
    Ok((record, header_len + status_len + text_len + 64))
}

impl TaskSnapshot<'_> {
    pub(crate) fn select(
        &self,
        sql: &str,
        params: Vec<Value>,
        max_rows: usize,
    ) -> Result<Vec<TaskRecord>> {
        self.context.check_bounds(max_rows, 0)?;
        self.step()?;
        let budget = self.context.limits.max_bytes;
        let mut statement = self.db.prepare(sql)?;
        let mut rows = statement.query(params_from_iter(params.iter()))?;
        let mut found: Vec<TaskRecord> = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next()? {
            let size = declared_size(row)?;
            // A record over the whole query budget is refused, never returned because the
            // batch is empty. A fitting record that would overflow a populated batch ends it.
            if size > budget {
                return Err(Error::LimitExceeded);
            }
            if bytes + size > budget {
                break;
            }
            let (next, size) = record(row)?;
            bytes += size;
            found.push(next);
            if found.len() >= max_rows {
                break;
            }
        }
        Ok(found)
    }

    /// Newest first by start time, then id, after the keyset position.
    pub fn batch(&self, query: &BatchQuery) -> Result<Vec<TaskRecord>> {
        let (sql, params) = statements::batch(query);
        self.select(&sql, params, query.max_rows)
    }

    /// Unfinished tasks, root-wide.
    pub fn unobserved_count(&self) -> Result<u64> {
        self.step()?;
        let count: i64 = self
            .db
            .query_row(&statements::unobserved_count(), [], |row| row.get(0))?;
        u64::try_from(count).map_err(|_| Error::MalformedRow)
    }

    pub fn unobserved_rows(
        &self,
        after: Option<(i64, String)>,
        max_rows: usize,
    ) -> Result<Vec<UnobservedRow>> {
        self.context.check_bounds(max_rows, 0)?;
        self.step()?;
        let mut params: Vec<Value> = Vec::new();
        let has_after = after.is_some();
        if let Some((stamp, id)) = after {
            params.push(Value::Integer(stamp));
            params.push(Value::Text(id));
        }
        params.push(Value::Integer(
            i64::try_from(max_rows).map_err(|_| Error::LimitExceeded)?,
        ));
        let mut statement = self.db.prepare(&statements::unobserved_rows(has_after))?;
        let mut rows = statement.query(params_from_iter(params.iter()))?;
        let mut found = Vec::new();
        let mut bytes = 0usize;
        while let Some(row) = rows.next()? {
            let session_len = within(row.get(9)?, 1024)?;
            let status_len = within(row.get(10)?, 256)?;
            let boot_len = within(row.get(11)?, 64)?;
            bytes += session_len + status_len + boot_len + 64;
            self.context.check_bounds(found.len() + 1, bytes)?;
            let process = match row.get::<_, Option<i64>>(5)? {
                Some(pid) => Some(process_record(pid, row.get(6)?, row.get(7)?, row.get(8)?)?),
                None => None,
            };
            found.push(UnobservedRow {
                id: task_id(row.get(0)?)?,
                stamp: row.get(1)?,
                session: row.get(2)?,
                status: row.get(3)?,
                exit_code: row.get(4)?,
                process,
            });
        }
        Ok(found)
    }
}

/// The bytes a record will hold, from the lengths the statement reports, before any value
/// is copied.
fn declared_size(row: &Row<'_>) -> Result<usize> {
    let header_len = within(row.get(21)?, BLOB_LIMIT)?;
    let status_len = within(row.get(22)?, BLOB_LIMIT)?;
    let text_len = within(row.get(23)?, i64::MAX)?;
    Ok(header_len + status_len + text_len + 64)
}
