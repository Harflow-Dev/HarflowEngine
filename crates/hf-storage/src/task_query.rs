use crate::{Error, Result, Store, connection, task_read};
use hf_application::task_read::TaskReadResult;
use hf_domain::task_read::{AuthorizedScope, Coverage, Lookup, PageFilter, TaskId, TaskQuery};
use rusqlite::{Connection, OptionalExtension, params_from_iter, types::Value};
use std::time::Instant;
pub(crate) fn execute(
    store: &Store,
    db: &Connection,
    scope: &AuthorizedScope,
    query: &TaskQuery,
    generation: &str,
    coverage: Coverage,
    deadline: Instant,
) -> Result<TaskReadResult> {
    match query {
        TaskQuery::Page {
            limit,
            filter,
            cursor,
        } => crate::task_page::page(
            store,
            db,
            scope,
            (*limit, filter, cursor.as_ref()),
            generation,
            coverage,
            deadline,
        ),
        TaskQuery::StoredStatus(id) => status_query(store, db, scope, id, coverage, deadline),
        TaskQuery::Ids(ids) => ids_query(store, db, scope, ids, coverage, deadline),
    }
}
fn scoped(scope: &AuthorizedScope) -> (String, Vec<Value>) {
    match scope {
        AuthorizedScope::Global => ("1".to_owned(), Vec::new()),
        AuthorizedScope::Closed(ids) => {
            let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            (
                format!("id IN ({placeholders})"),
                ids.iter()
                    .map(|id| Value::Text(id.as_str().to_owned()))
                    .collect(),
            )
        }
    }
}
pub(crate) fn filtered(scope: &AuthorizedScope, filter: &PageFilter) -> (String, Vec<Value>) {
    let (mut sql, mut params) = scoped(scope);
    if filter.active_only {
        sql.push_str(" AND active=1");
    }
    if let Some(session) = &filter.session {
        sql.push_str(" AND session=?");
        params.push(Value::Text(session.clone()));
    }
    (sql, params)
}
pub(crate) fn empty(coverage: Coverage) -> TaskReadResult {
    TaskReadResult {
        rows: Vec::new(),
        lookups: Vec::new(),
        cursor: None,
        count: None,
        activity_complete: true,
        coverage,
        partial: false,
    }
}
fn ids_query(
    store: &Store,
    db: &Connection,
    scope: &AuthorizedScope,
    ids: &[TaskId],
    coverage: Coverage,
    deadline: Instant,
) -> Result<TaskReadResult> {
    store.check_bounds(ids.len(), ids.iter().map(|id| id.as_str().len()).sum())?;
    let mut result = empty(coverage);
    let mut bytes = 512usize;
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        connection::timeout(db, deadline)?;
        let lookup = lookup(db, scope, id, coverage, deadline)?;
        if let Lookup::Found(found) = &lookup {
            let row = task_read::materialize(db, found, deadline)?;
            bytes = bytes
                .checked_add(task_read::row_bytes(&row))
                .ok_or(Error::LimitExceeded)?;
            store.check_bounds(result.rows.len() + 1, bytes)?;
            if bytes > 128 * 1024 {
                return Err(Error::LimitExceeded);
            }
            result.activity_complete &= row.observation.active.is_some();
            result.rows.push(row);
        } else {
            result.activity_complete = false;
        }
        bytes = bytes
            .checked_add(lookup_bytes(&lookup))
            .ok_or(Error::LimitExceeded)?;
        store.check_bounds(result.lookups.len() + 1, bytes)?;
        if bytes > 128 * 1024 {
            return Err(Error::LimitExceeded);
        }
        result.lookups.push(lookup);
    }
    Ok(result)
}
fn lookup(
    db: &Connection,
    scope: &AuthorizedScope,
    id: &TaskId,
    coverage: Coverage,
    deadline: Instant,
) -> Result<Lookup> {
    if scope.permits(id) {
        let exact: Option<String> = db
            .query_row(
                "SELECT id FROM task_headers WHERE id=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if exact.is_some() {
            return Ok(Lookup::Found(id.clone()));
        }
        connection::timeout(db, deadline)?;
        let pending: Option<String> = db
            .query_row(
                "SELECT id FROM task_pending WHERE id=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if pending.is_some() {
            return Ok(Lookup::Pending(id.clone()));
        }
    }
    if id.canonical() {
        return Ok(Lookup::Missing);
    }
    if !id.may_prefix() {
        return Ok(Lookup::InvalidPrefix);
    }
    if !coverage.discovery_complete || !coverage.ownership_complete || !coverage.source_complete {
        return Ok(Lookup::Incomplete);
    }
    let (predicate, mut params) = scoped(scope);
    params.push(Value::Text(id.as_str().to_owned()));
    params.push(Value::Text(format!("{}~", id.as_str())));
    connection::timeout(db, deadline)?;
    let mut statement = db.prepare(&format!(
        "SELECT CASE WHEN typeof(id)='text' THEN CAST(substr(CAST(id AS BLOB),1,65) AS TEXT) END FROM task_headers WHERE {predicate} AND id>=? AND id<? ORDER BY id LIMIT 6"
    ))?;
    let mut rows = statement.query(params_from_iter(params.iter()))?;
    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        connection::check_deadline(deadline)?;
        candidates.push(TaskId::new(row.get(0)?).map_err(|_| Error::Incompatible)?);
    }
    if candidates.len() == 1 {
        return Ok(Lookup::Found(candidates.remove(0)));
    }
    if candidates.is_empty() {
        return Ok(Lookup::Missing);
    }
    let truncated = candidates.len() > 5;
    candidates.truncate(5);
    Ok(Lookup::Ambiguous {
        candidates,
        truncated,
    })
}

fn status_query(
    store: &Store,
    db: &Connection,
    scope: &AuthorizedScope,
    id: &TaskId,
    coverage: Coverage,
    deadline: Instant,
) -> Result<TaskReadResult> {
    ids_query(
        store,
        db,
        scope,
        std::slice::from_ref(id),
        coverage,
        deadline,
    )
}

fn lookup_bytes(lookup: &Lookup) -> usize {
    match lookup {
        Lookup::Found(id) | Lookup::Pending(id) => 16 + id.as_str().len(),
        Lookup::Ambiguous { candidates, .. } => {
            16 + candidates
                .iter()
                .map(|id| id.as_str().len() + 8)
                .sum::<usize>()
        }
        _ => 16,
    }
}
