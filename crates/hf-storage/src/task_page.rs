use crate::task_query::{empty, filtered};
use crate::{Error, Result, connection, read_context::ReadContext, task_read};
use hf_application::task_read::TaskReadResult;
use hf_domain::task_read::{AuthorizedScope, Coverage, Cursor, PageFilter, SortKey, TaskId};
use rusqlite::{Connection, params_from_iter, types::Value};
use std::time::Instant;
pub(crate) fn page(
    context: &ReadContext<'_>,
    db: &Connection,
    scope: &AuthorizedScope,
    args: (usize, &PageFilter, Option<&Cursor>),
    generation: &str,
    coverage: Coverage,
    deadline: Instant,
) -> Result<TaskReadResult> {
    let (limit, filter, cursor) = args;
    if let Some(cursor) = cursor
        && (cursor.store != context.identity.generation
            || cursor.dataset != generation
            || cursor.scope != *scope
            || cursor.filter != *filter)
    {
        return Err(Error::StaleTaskCursor);
    }
    let (predicate, params) = filtered(scope, filter);
    connection::timeout(db, deadline)?;
    let matching_count: i64 = db.query_row(
        &format!("SELECT count(*) FROM task_headers WHERE {predicate}"),
        params_from_iter(params.iter()),
        |r| r.get(0),
    )?;
    let unknown_filter = PageFilter {
        active_only: false,
        session: filter.session.clone(),
    };
    let (unknown_predicate, unknown_params) = filtered(scope, &unknown_filter);
    connection::timeout(db, deadline)?;
    let unknown: i64 = db.query_row(
        &format!("SELECT count(*) FROM task_headers WHERE {unknown_predicate} AND (active IS NULL OR typeof(active)!='integer' OR active NOT IN(0,1))"),
        params_from_iter(unknown_params.iter()),
        |r| r.get(0),
    )?;
    let (sql, params) = page_sql(scope, filter, cursor, limit);
    connection::timeout(db, deadline)?;
    let mut statement = db.prepare(&sql)?;
    let mut candidates = statement.query(params_from_iter(params.iter()))?;
    let mut result = empty(coverage);
    let cursor_bytes = match scope {
        AuthorizedScope::Global => 0,
        AuthorizedScope::Closed(ids) => ids.iter().map(|id| id.as_str().len() + 8).sum(),
    };
    let reserve = 512 + cursor_bytes + filter.session.as_ref().map_or(0, String::len);
    result.activity_complete = unknown == 0
        && coverage.discovery_complete
        && coverage.ownership_complete
        && coverage.source_complete;
    collect(
        context,
        db,
        &mut candidates,
        &mut result,
        limit,
        reserve,
        deadline,
    )?;
    if coverage.discovery_complete
        && coverage.ownership_complete
        && coverage.source_complete
        && unknown == 0
    {
        result.count = Some(usize::try_from(matching_count).map_err(|_| Error::Incompatible)?);
    }
    if result.partial {
        result.cursor = result.rows.last().map(|row| Cursor {
            store: context.identity.generation.clone(),
            dataset: generation.to_owned(),
            scope: scope.clone(),
            filter: filter.clone(),
            after: SortKey {
                stamp: row.stamp,
                id: row.id.clone(),
            },
        });
    }
    Ok(result)
}
fn collect(
    context: &ReadContext<'_>,
    db: &Connection,
    candidates: &mut rusqlite::Rows<'_>,
    result: &mut TaskReadResult,
    limit: usize,
    reserve: usize,
    deadline: Instant,
) -> Result<()> {
    let mut bytes = reserve;
    let cap = context.limits.max_bytes.min(128 * 1024);
    while let Some(row) = candidates.next()? {
        connection::check_deadline(deadline)?;
        if result.rows.len() >= limit.min(context.limits.max_rows) {
            if result.rows.is_empty() {
                return Err(Error::LimitExceeded);
            }
            result.partial = true;
            break;
        }
        let id = TaskId::new(row.get(0)?).map_err(|_| Error::Incompatible)?;
        let row = task_read::materialize(db, &id, deadline)?;
        let next = bytes
            .checked_add(task_read::row_bytes(&row))
            .ok_or(Error::LimitExceeded)?;
        if next > cap {
            if result.rows.is_empty() {
                return Err(Error::LimitExceeded);
            }
            result.partial = true;
            break;
        }
        bytes = next;
        result.rows.push(row);
    }
    Ok(())
}

/// Shared production query builder, also inspected by snapshot query-plan tests.
pub(crate) fn page_sql(
    scope: &AuthorizedScope,
    filter: &PageFilter,
    cursor: Option<&Cursor>,
    limit: usize,
) -> (String, Vec<Value>) {
    let (predicate, mut params) = filtered(scope, filter);
    let mut sql = format!(
        "SELECT CASE WHEN typeof(id)='text' THEN CAST(substr(CAST(id AS BLOB),1,65) AS TEXT) END FROM task_headers WHERE {predicate}"
    );
    if let Some(cursor) = cursor {
        sql.push_str(" AND (stamp,id) < (?,?)");
        params.extend([
            Value::Integer(cursor.after.stamp),
            Value::Text(cursor.after.id.as_str().to_owned()),
        ]);
    }
    sql.push_str(" ORDER BY stamp DESC,id DESC LIMIT ?");
    params.push(Value::Integer((limit + 1) as i64));
    (sql, params)
}
