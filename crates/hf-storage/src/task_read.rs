use crate::{Error, Result, Store, connection, read_context::ReadContext};
use hf_application::task_read::{TaskReadError, TaskReadPort, TaskReadResult, TaskRow};
use hf_domain::task_read::{AuthorizedScope, Coverage, StoredObservation, TaskId, TaskQuery};
use rusqlite::{Connection, OptionalExtension};
use std::time::Instant;
pub(crate) const DATASET_QUERY: &str = "SELECT version,CASE WHEN typeof(generation)='text' THEN CAST(substr(CAST(generation AS BLOB),1,33) AS TEXT) END,discovery,ownership,source FROM task_dataset WHERE singleton=1";

pub struct TaskReadSession<'a> {
    store: &'a Store,
}
impl<'a> TaskReadSession<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }
}
impl TaskReadPort for TaskReadSession<'_> {
    fn execute(
        &self,
        scope: &AuthorizedScope,
        query: &TaskQuery,
        deadline: Instant,
    ) -> std::result::Result<TaskReadResult, TaskReadError> {
        scope.validate().map_err(|_| TaskReadError::Refused)?;
        query.validate().map_err(TaskReadError::InvalidQuery)?;
        self.store
            .task_read(scope, query, deadline)
            .map_err(|error| match error {
                Error::TaskReadUnavailable => TaskReadError::Unavailable,
                Error::InvalidTaskQuery => {
                    TaskReadError::InvalidQuery(hf_domain::task_read::QueryError::InvalidScope)
                }
                Error::StaleTaskCursor => {
                    TaskReadError::InvalidQuery(hf_domain::task_read::QueryError::StaleCursor)
                }
                _ => TaskReadError::Storage,
            })
    }
}
impl Store {
    pub fn task_read(
        &self,
        scope: &AuthorizedScope,
        query: &TaskQuery,
        deadline: Instant,
    ) -> Result<TaskReadResult> {
        query.validate().map_err(|_| Error::InvalidTaskQuery)?;
        scope.validate().map_err(|_| Error::InvalidTaskQuery)?;
        let mut connection = self.admit(deadline)?;
        let transaction = connection.transaction()?;
        let result = run(&self.read_context(), &transaction, scope, query, deadline)?;
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(result)
    }
}
/// The queries of one task read, shared by every session that can answer it.
/// The caller owns admission, the transaction and the closing checks.
pub(crate) fn run(
    context: &ReadContext<'_>,
    db: &Connection,
    scope: &AuthorizedScope,
    query: &TaskQuery,
    deadline: Instant,
) -> Result<TaskReadResult> {
    connection::timeout(db, deadline)?;
    let (generation, coverage) = dataset(db)?;
    #[cfg(all(test, unix))]
    crate::tests::stage("task_read_snapshot");
    crate::task_query::execute(context, db, scope, query, &generation, coverage, deadline)
}
pub(crate) fn require_tables(connection: &Connection) -> Result<()> {
    let count: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('task_dataset','task_headers','task_pending')",[],|r|r.get(0))?;
    if count == 3 {
        Ok(())
    } else {
        Err(Error::TaskReadUnavailable)
    }
}
fn dataset(connection: &Connection) -> Result<(String, Coverage)> {
    dataset_optional(connection)?.ok_or(Error::TaskReadUnavailable)
}
pub(crate) fn dataset_optional(connection: &Connection) -> Result<Option<(String, Coverage)>> {
    require_tables(connection)?;
    let data: Option<(u32, String, i64, i64, i64)> = connection
        .query_row(DATASET_QUERY, [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?;
    let Some((version, generation, discovery, ownership, source)) = data else {
        return Ok(None);
    };
    if version != 1 || generation.len() != 32 || !generation.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::TaskReadUnavailable);
    }
    Ok(Some((
        generation,
        Coverage {
            discovery_complete: strict_bool(discovery)?,
            ownership_complete: strict_bool(ownership)?,
            source_complete: strict_bool(source)?,
        },
    )))
}
pub(crate) fn row_bytes(row: &TaskRow) -> usize {
    row.id.as_str().len()
        + row.session.as_ref().map_or(0, String::len)
        + row
            .observation
            .recorded_status
            .as_ref()
            .map_or(0, String::len)
        + row.header.len()
        + row.status.as_ref().map_or(0, Vec::len)
        + 32
}
pub(crate) fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    let raw: String = row.get(0)?;
    let id = TaskId::new(raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(TaskRow {
        id,
        stamp: row.get(1)?,
        session: row.get(2)?,
        observation: StoredObservation {
            active: optional_bool(row, 3)?,
            recorded_status: row.get(4)?,
            exited: optional_bool(row, 5)?,
            drained: optional_bool(row, 6)?,
            owned: optional_bool(row, 7)?,
        },
        header: row.get(8)?,
        status: row.get(9)?,
    })
}
pub(crate) fn materialize(
    connection: &Connection,
    id: &TaskId,
    deadline: Instant,
) -> Result<TaskRow> {
    connection::timeout(connection, deadline)?;
    let lengths: (i64, Option<i64>, i64, i64) = connection.query_row(
        "SELECT length(header),length(status),coalesce(length(CAST(session AS BLOB)),0),coalesce(length(CAST(recorded_status AS BLOB)),0) FROM task_headers WHERE id=?1",
        [id.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if lengths.0 < 0
        || lengths.0 > 2048
        || lengths.1.is_some_and(|n| !(0..=2048).contains(&n))
        || !(0..=1024).contains(&lengths.2)
        || !(0..=256).contains(&lengths.3)
    {
        return Err(Error::LimitExceeded);
    }
    connection::timeout(connection, deadline)?;
    let row = connection.query_row("SELECT id,stamp,session,active,recorded_status,exited,drained,owned,header,status FROM task_headers WHERE id=?1",[id.as_str()],decode)?;
    connection::check_deadline(deadline)?;
    Ok(row)
}

fn strict_bool(value: i64) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::Incompatible),
    }
}
fn optional_bool(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<bool>> {
    let value: Option<i64> = row.get(index)?;
    value
        .map(|value| match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(rusqlite::Error::InvalidQuery),
        })
        .transpose()
}
