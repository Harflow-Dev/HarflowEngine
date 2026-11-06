use crate::{Error, Result, connection, event_read::Stream, read_context::ReadContext};
use hf_application::event_read::{EventReadResult, EventRecord, RecordClass};
use hf_domain::{
    event_read::{Direction, EventQuery},
    task_read::TaskId,
};
use rusqlite::{Connection, OptionalExtension};
use std::time::Instant;
const WINDOW: u64 = 1_048_576;
pub(crate) struct Lines {
    pub max_rows: usize,
    pub max_bytes: usize,
    pub rows: Vec<Line>,
    pub older: u64,
    pub newer: u64,
    pub older_open: bool,
    pub newer_open: bool,
    pub has_newer: bool,
    pub direction: Direction,
}
pub(crate) struct Line {
    pub record: EventRecord,
    pub open_start: bool,
    pub open_end: bool,
}
impl Line {
    fn synthetic(
        start: u64,
        end: u64,
        class: RecordClass,
        open_start: bool,
        open_end: bool,
    ) -> Self {
        Self {
            record: EventRecord {
                start,
                end,
                class,
                seq: None,
                kind: None,
                body: Vec::new(),
            },
            open_start,
            open_end,
        }
    }
}
pub(crate) fn read(
    context: &ReadContext<'_>,
    db: &Connection,
    query: &EventQuery,
    task: &TaskId,
    stream: &Stream,
    result: &mut EventReadResult,
    deadline: Instant,
) -> Result<()> {
    let direction = query.cursor.as_ref().map_or(
        if query.from_oldest {
            Direction::Newer
        } else {
            Direction::Older
        },
        |c| c.direction,
    );
    let boundary = query.cursor.as_ref().map_or(
        if direction == Direction::Older {
            stream.size()
        } else {
            0
        },
        |c| c.boundary,
    );
    let resync = query.cursor.as_ref().is_some_and(|c| c.resync);
    context.check_bounds(1, 1024)?;
    let mut lines = Lines {
        max_rows: context.limits.max_rows,
        max_bytes: context.limits.max_bytes.min(262_144),
        rows: Vec::new(),
        older: if direction == Direction::Older {
            0
        } else {
            boundary
        },
        newer: if direction == Direction::Older && query.cursor.is_none() {
            0
        } else {
            boundary
        },
        older_open: direction == Direction::Newer && resync,
        newer_open: resync,
        has_newer: false,
        direction,
    };
    connection::timeout(db, deadline)?;
    let (start, stop) = if direction == Direction::Older {
        (boundary.saturating_sub(WINDOW), boundary)
    } else {
        (boundary, stream.size().min(boundary.saturating_add(WINDOW)))
    };
    result.scanned_bytes = stop - start;
    if direction == Direction::Older {
        backward(
            db, query, task, start, stop, resync, &mut lines, result, deadline,
        )?;
        lines.has_newer = query.cursor.is_some()
            && next_end(
                db,
                task,
                boundary,
                stream.size().min(boundary.saturating_add(WINDOW)),
            )?
            .is_some();
    } else {
        forward(
            db,
            query,
            task,
            start,
            stop,
            stream.size(),
            resync,
            &mut lines,
            result,
            deadline,
        )?;
    }
    crate::event_render::render(context, query, task, stream, lines, result);
    Ok(())
}
fn next_end(db: &Connection, task: &TaskId, after: u64, stop: u64) -> Result<Option<u64>> {
    Ok(db.query_row("SELECT end FROM event_records WHERE task=?1 AND end>?2 AND end<=?3 ORDER BY end LIMIT 1",rusqlite::params![task.as_str(),after as i64,stop as i64],|r|r.get::<_,i64>(0)).optional()?.map(|v|v as u64))
}
fn last_end(db: &Connection, task: &TaskId, start: u64, stop: u64) -> Result<Option<u64>> {
    Ok(db.query_row("SELECT end FROM event_records WHERE task=?1 AND end>?2 AND end<=?3 ORDER BY end DESC LIMIT 1",rusqlite::params![task.as_str(),start as i64,stop as i64],|r|r.get::<_,i64>(0)).optional()?.map(|v|v as u64))
}
#[allow(clippy::too_many_arguments)]
fn backward(
    db: &Connection,
    q: &EventQuery,
    task: &TaskId,
    start: u64,
    stop: u64,
    resync: bool,
    lines: &mut Lines,
    out: &mut EventReadResult,
    deadline: Instant,
) -> Result<()> {
    let Some(last) = last_end(db, task, start, stop)? else {
        if start > 0 {
            lines.rows.push(Line::synthetic(
                start,
                stop,
                RecordClass::Oversized,
                true,
                true,
            ));
        }
        return Ok(());
    };
    let first = if start > 0 {
        next_end(db, task, start, stop)?.ok_or(Error::Incompatible)?
    } else {
        0
    };
    if first >= last {
        if last == stop {
            lines.rows.push(Line::synthetic(
                start,
                last,
                RecordClass::Oversized,
                true,
                false,
            ));
        } else if resync {
            lines.rows.push(Line::synthetic(
                last,
                stop,
                RecordClass::Oversized,
                false,
                true,
            ));
        } else {
            lines.rows.push(Line::synthetic(
                last,
                last,
                RecordClass::Blank,
                false,
                false,
            ));
            out.decoded += 1;
        }
        return Ok(());
    }
    collect(db, q, task, first, last, true, lines, out, deadline)?;
    lines.rows.reverse();
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn forward(
    db: &Connection,
    q: &EventQuery,
    task: &TaskId,
    begin: u64,
    stop: u64,
    size: u64,
    resync: bool,
    lines: &mut Lines,
    out: &mut EventReadResult,
    deadline: Instant,
) -> Result<()> {
    let mut position = begin;
    if resync {
        if let Some(end) = next_end(db, task, begin, stop)? {
            lines.rows.push(Line::synthetic(
                begin,
                end,
                RecordClass::Oversized,
                true,
                false,
            ));
            position = end;
        } else {
            if stop > begin {
                lines.rows.push(Line::synthetic(
                    begin,
                    stop,
                    RecordClass::Oversized,
                    true,
                    true,
                ));
            }
            lines.has_newer = stop != size;
            return Ok(());
        }
    }
    collect(db, q, task, position, stop, false, lines, out, deadline)?;
    position = lines.rows.last().map_or(position, |l| l.record.end);
    let next = next_end(db, task, position, stop)?;
    if lines.rows.is_empty() && stop < size && next_end(db, task, begin, stop)?.is_none() {
        lines.rows.push(Line::synthetic(
            begin,
            stop,
            RecordClass::Oversized,
            false,
            true,
        ));
    }
    lines.has_newer = next.is_some() || stop < size;
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn collect(
    db: &Connection,
    q: &EventQuery,
    task: &TaskId,
    begin: u64,
    stop: u64,
    reverse: bool,
    lines: &mut Lines,
    out: &mut EventReadResult,
    deadline: Instant,
) -> Result<()> {
    let kinds = q.filter.kinds();
    let mut boundary = if reverse { stop } else { begin };
    let mut bytes = 1024usize;
    while out.decoded < q.limit && lines.rows.len() < lines.max_rows {
        connection::timeout(db, deadline)?;
        let record = fetch(db, task, begin, stop, boundary, reverse)?;
        let Some((mut record, length)) = record else {
            break;
        };
        let retained = record.class == RecordClass::Valid
            && record.kind.as_ref().is_some_and(|k| kinds.contains(k));
        let cost = 256 + if retained { length } else { 0 };
        if bytes + cost > lines.max_bytes {
            if lines.rows.is_empty() {
                return Err(Error::LimitExceeded);
            }
            break;
        }
        bytes += cost;
        boundary = if reverse { record.start } else { record.end };
        if record.class != RecordClass::Oversized {
            out.decoded += 1;
        }
        if retained {
            connection::timeout(db, deadline)?;
            record.body = db.query_row(
                "SELECT substr(body,1,16385) FROM event_records WHERE task=?1 AND start=?2",
                rusqlite::params![task.as_str(), record.start as i64],
                |r| r.get(0),
            )?;
            crate::event_fixture::validate_record(&record)?;
        }
        lines.rows.push(Line {
            record,
            open_start: false,
            open_end: false,
        });
    }
    Ok(())
}
fn fetch(
    db: &Connection,
    task: &TaskId,
    begin: u64,
    stop: u64,
    boundary: u64,
    reverse: bool,
) -> Result<Option<(EventRecord, usize)>> {
    let sql = seek_sql(reverse);
    let raw = db
        .query_row(
            &sql,
            rusqlite::params![task.as_str(), begin as i64, stop as i64, boundary as i64],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((start, end, class, seq, kind, length, kind_type)) = raw else {
        return Ok(None);
    };
    if length > 16384 || kind_type != "blob" {
        return Err(Error::LimitExceeded);
    }
    if start < 0 || end < 0 || seq.is_some_and(|s| s < 0) {
        return Err(Error::Incompatible);
    }
    let class = match class {
        0 => RecordClass::Valid,
        1 => RecordClass::Blank,
        2 => RecordClass::Malformed,
        3 => RecordClass::Oversized,
        _ => return Err(Error::Incompatible),
    };
    if class != RecordClass::Valid && length != 0 {
        return Err(Error::Incompatible);
    }
    let record = EventRecord {
        start: start as u64,
        end: end as u64,
        class,
        seq: seq.map(|s| s as u64),
        kind,
        body: Vec::new(),
    };
    crate::event_fixture::validate_record(&record)?;
    Ok(Some((record, length as usize)))
}

pub(crate) fn seek_sql(reverse: bool) -> String {
    let order = if reverse {
        "start>=?2 AND end<=?4 ORDER BY end DESC"
    } else {
        "start>=?4 AND end<=?3 ORDER BY start"
    };
    let index = if reverse {
        "event_ends"
    } else {
        "event_starts"
    };
    format!(
        "SELECT start,end,class,seq,CASE WHEN typeof(kind)='text' THEN CAST(substr(CAST(kind AS BLOB),1,129) AS TEXT) WHEN kind IS NULL THEN NULL ELSE 1 END,length(body),typeof(body) FROM event_records INDEXED BY {index} WHERE task=?1 AND {order} LIMIT 1"
    )
}
