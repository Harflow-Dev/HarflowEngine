use crate::{Error, Result};
use rusqlite::Connection;
pub(crate) const SCHEMA: &str = "CREATE TABLE task_watch_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,revision INTEGER NOT NULL); INSERT INTO task_watch_meta VALUES(1,1,0);";
/// Scalar projections and LIMIT 2 bound allocation even for malformed tables/cells.
pub(crate) fn revision(connection: &Connection) -> Result<i64> {
    let mut statement = connection.prepare("SELECT CASE WHEN typeof(singleton)='integer' THEN singleton END, CASE WHEN typeof(version)='integer' THEN version END, CASE WHEN typeof(revision)='integer' THEN revision END FROM task_watch_meta LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows.next()?.ok_or(Error::TaskWatchUnavailable)?;
    let singleton: Option<i64> = row.get(0)?;
    let version: Option<i64> = row.get(1)?;
    let revision: Option<i64> = row.get(2)?;
    let revision = revision
        .filter(|value| *value >= 0)
        .ok_or(Error::TaskWatchUnavailable)?;
    if singleton != Some(1) || version != Some(1) || rows.next()?.is_some() {
        return Err(Error::TaskWatchUnavailable);
    }
    Ok(revision)
}
pub(crate) fn publish(connection: &Connection) -> Result<()> {
    let current = revision(connection)?;
    let next = current.checked_add(1).ok_or(Error::TaskWatchUnavailable)?;
    if connection.execute(
        "UPDATE task_watch_meta SET revision=?1 WHERE singleton=1",
        [next],
    )? != 1
    {
        return Err(Error::TaskWatchUnavailable);
    }
    Ok(())
}
