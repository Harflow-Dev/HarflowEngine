//! Fixture mutation validation. Reads never repair persisted data.
use crate::{Error, Result, Store, connection, event_read::Stream};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::task_read::TaskId;
use rusqlite::Connection;
use std::time::Instant;
pub(crate) fn existing(
    store: &Store,
    db: &Connection,
    task: &TaskId,
    stream: Option<&Stream>,
    deadline: Instant,
) -> Result<(usize, usize)> {
    let limit =
        i64::try_from(store.limits.max_rows.saturating_add(1)).map_err(|_| Error::LimitExceeded)?;
    let mut statement=db.prepare("SELECT start,end,class,seq,CASE WHEN kind IS NULL THEN NULL WHEN typeof(kind)='text' THEN CAST(substr(CAST(kind AS BLOB),1,129) AS TEXT) ELSE 1 END,CASE WHEN typeof(body)='blob' AND length(body)<=16384 THEN body END FROM event_records INDEXED BY event_starts WHERE task=?1 ORDER BY start LIMIT ?2")?;
    let mut rows = statement.query(rusqlite::params![task.as_str(), limit])?;
    let mut previous = 0u64;
    let mut count = 0usize;
    let mut bytes = 512usize;
    while let Some(row) = rows.next()? {
        connection::check_deadline(deadline)?;
        let start: i64 = row.get(0)?;
        let end: i64 = row.get(1)?;
        let seq: Option<i64> = row.get(3)?;
        if start < 0 || end < 0 || seq.is_some_and(|s| s < 0) {
            return Err(Error::Incompatible);
        }
        let class = match row.get::<_, i64>(2)? {
            0 => RecordClass::Valid,
            1 => RecordClass::Blank,
            2 => RecordClass::Malformed,
            3 => RecordClass::Oversized,
            _ => return Err(Error::Incompatible),
        };
        let record = EventRecord {
            start: start as u64,
            end: end as u64,
            class,
            seq: seq.map(|s| s as u64),
            kind: row.get(4)?,
            body: row.get(5)?,
        };
        crate::event_fixture::validate_record(&record)?;
        if record.start != previous {
            return Err(Error::Incompatible);
        }
        previous = record.end;
        count += 1;
        bytes = bytes
            .checked_add(record.body.len() + record.kind.as_ref().map_or(0, String::len) + 64)
            .ok_or(Error::LimitExceeded)?;
        store.check_bounds(count, bytes)?;
    }
    if stream.is_none_or(|s| s.complete_end != previous) && (count > 0 || stream.is_some()) {
        return Err(Error::Incompatible);
    }
    Ok((count, bytes))
}
