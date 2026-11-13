//! The production event writer: appends terminated event lines to a valid task's log in one
//! commit. The stream generation and cursors survive an append, and nothing is published to
//! the task list (event writes do not bump the watch revision).
use crate::{
    Error, Result, Store, connection, event_fixture, event_read, task_writer::task_valid,
    write_tx::Publish,
};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::{event_read::LogFacts, task_read::TaskId};
use rusqlite::{OptionalExtension, Transaction};
use std::time::Instant;

/// Largest stored event body (the projection of the raw line), in bytes.
pub const EVENT_BODY_LIMIT: usize = 16_384;
/// Longest raw event line without its newline; a longer line is stored as `Oversized`.
pub const EVENT_LINE_LIMIT: u64 = 1_000_000;
/// Longest event kind, in bytes.
pub const EVENT_KIND_LIMIT: usize = 128;
/// Longest `task_status` a new stream stores, in bytes.
const STATUS_LIMIT: usize = 256;

/// One terminated event line of the engine's own event log. `raw_len` is the raw line's length
/// without its newline; the record spans `raw_len + 1` bytes. `body` is the projected event the
/// reads serve, at most 16,384 bytes of UTF-8; it is not tied to `raw_len` (a projection can be
/// shorter or longer than the raw line). A line over 1,000,000 bytes is stored as `Oversized`:
/// its `seq`, `kind` and `body` are then neither checked nor stored.
///
/// The writer stores only `Valid` and `Oversized` records, so it cannot ingest a foreign log
/// with blank or malformed lines; those stay fixture damage writers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventAppend {
    pub seq: u64,
    pub kind: String,
    pub raw_len: u64,
    pub body: Vec<u8>,
}

impl Store {
    /// Appends one event line; see `append_events`.
    pub fn append_event(
        &self,
        id: &TaskId,
        event: &EventAppend,
        facts_if_new: &LogFacts,
        deadline: Instant,
    ) -> Result<()> {
        self.append_events(id, std::slice::from_ref(event), facts_if_new, deadline)
    }
    /// Appends the lines in order, in one commit, at the stream's `complete_end`. When the task
    /// has no stream yet, one is created with `facts_if_new` and a fresh generation; an existing
    /// stream keeps its generation and facts, and only its `complete_end` moves; `facts_if_new`
    /// is then neither checked nor stored. An empty batch writes nothing and commits nothing.
    ///
    /// Refused with nothing committed (the transaction rolls back): `TaskMissing` for a missing or damaged task;
    /// `EventStreamClosed` for an existing stream that stores an EOF seq (its task finished);
    /// `LimitExceeded` for a body over 16,384 bytes, a kind over 128 bytes, a seq beyond
    /// `i64::MAX`, a span past `i64::MAX`, or, when creating the stream, `facts_if_new` with a
    /// status over 256 bytes or an EOF seq beyond `i64::MAX`; `InvalidEventQuery` for an empty
    /// line, a body that is not UTF-8, or, when creating the stream, `facts_if_new` naming an
    /// EOF seq the batch does not end on as a `task_finished` event with that seq;
    /// `EventOutOfOrder` for a seq not above the last valid record's (within the batch too);
    /// `Incompatible` for a stream with a partial tail or whose last record does not end at
    /// `complete_end`. Event bounds are checked before the transaction opens; a span past
    /// `i64::MAX` and `EventOutOfOrder` are found while the batch is inserted.
    pub fn append_events(
        &self,
        id: &TaskId,
        events: &[EventAppend],
        facts_if_new: &LogFacts,
        deadline: Instant,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        // Refuses a malformed batch without opening a transaction; `append_in` checks again.
        for event in events {
            check_event(event)?;
        }
        self.write(deadline, Publish::Nothing, None, |tx| {
            append_in(tx, id, events, facts_if_new, deadline).map(|_| ())
        })
    }
}

/// The append inside a caller-owned immediate transaction. Every event passes `check_event`
/// before its row is inserted. Returns the stream as stored afterwards.
pub(crate) fn append_in(
    tx: &Transaction<'_>,
    id: &TaskId,
    events: &[EventAppend],
    facts_if_new: &LogFacts,
    deadline: Instant,
) -> Result<event_read::Stream> {
    event_fixture::require_schema(tx)?;
    if task_valid(tx, id)? != Some(true) {
        return Err(Error::TaskMissing);
    }
    let previous = event_read::stream_optional(tx, id)?;
    if previous.is_some() {
        if stored_eof(tx, id)? {
            return Err(Error::EventStreamClosed);
        }
    } else {
        check_new_stream(facts_if_new, events)?;
    }
    let begin = previous.as_ref().map_or(0, |s| s.complete_end);
    if previous.as_ref().is_some_and(|s| s.partial_tail != 0) || last_end(tx, id)? != begin {
        return Err(Error::Incompatible);
    }
    let mut last_seq = last_valid_seq(tx, id)?;
    let mut start = begin;
    for event in events {
        connection::timeout(tx, deadline)?;
        check_event(event)?;
        let record = record(start, event)?;
        if let Some(seq) = record.seq {
            if last_seq.is_some_and(|last| seq <= last) {
                return Err(Error::EventOutOfOrder);
            }
            last_seq = Some(seq);
        }
        insert(tx, id, &record)?;
        start = record.end;
    }
    let end = i64::try_from(start).map_err(|_| Error::LimitExceeded)?;
    if previous.is_some() {
        tx.execute(
            "UPDATE event_streams SET complete_end=?2 WHERE task=?1",
            rusqlite::params![id.as_str(), end],
        )?;
    } else {
        let generation: String =
            tx.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?;
        event_fixture::write_stream(tx, id, facts_if_new, start, 0, &generation)?;
    }
    // Defensive: the re-read applies every stream check the reads apply.
    event_read::stream_optional(tx, id)?.ok_or(Error::EventReadUnavailable)
}

/// The per-event bounds.
pub(crate) fn check_event(event: &EventAppend) -> Result<()> {
    if event.raw_len > EVENT_LINE_LIMIT {
        return Ok(());
    }
    if event.raw_len == 0 || std::str::from_utf8(&event.body).is_err() {
        return Err(Error::InvalidEventQuery);
    }
    if event.body.len() > EVENT_BODY_LIMIT
        || event.kind.len() > EVENT_KIND_LIMIT
        || event.seq > i64::MAX as u64
    {
        return Err(Error::LimitExceeded);
    }
    Ok(())
}
/// The facts of a stream the append creates: within their bounds, and an EOF seq only for a
/// batch that ends on the `task_finished` event with that seq.
fn check_new_stream(facts: &LogFacts, events: &[EventAppend]) -> Result<()> {
    if facts
        .task_status
        .as_ref()
        .is_some_and(|s| s.len() > STATUS_LIMIT)
        || facts.eof_finished_seq.is_some_and(|s| s > i64::MAX as u64)
    {
        return Err(Error::LimitExceeded);
    }
    if let Some(eof) = facts.eof_finished_seq {
        let closes = events.last().is_some_and(|last| {
            last.raw_len <= EVENT_LINE_LIMIT && last.kind == "task_finished" && last.seq == eof
        });
        if !closes {
            return Err(Error::InvalidEventQuery);
        }
    }
    Ok(())
}
/// Whether the stream row stores an EOF seq. The column is read as stored, not as the
/// stream read derives it from the last record.
fn stored_eof(tx: &Transaction<'_>, id: &TaskId) -> Result<bool> {
    let eof: Option<i64> = tx.query_row(
        "SELECT eof_seq FROM event_streams WHERE task=?1",
        [id.as_str()],
        |r| r.get(0),
    )?;
    Ok(eof.is_some())
}
fn record(start: u64, event: &EventAppend) -> Result<EventRecord> {
    let end = event
        .raw_len
        .checked_add(1)
        .and_then(|span| start.checked_add(span))
        .filter(|end| *end <= i64::MAX as u64)
        .ok_or(Error::LimitExceeded)?;
    let record = if event.raw_len > EVENT_LINE_LIMIT {
        EventRecord {
            start,
            end,
            class: RecordClass::Oversized,
            seq: None,
            kind: None,
            body: Vec::new(),
        }
    } else {
        EventRecord {
            start,
            end,
            class: RecordClass::Valid,
            seq: Some(event.seq),
            kind: Some(event.kind.clone()),
            body: event.body.clone(),
        }
    };
    event_fixture::validate_record(&record)?;
    Ok(record)
}
fn insert(tx: &Transaction<'_>, id: &TaskId, record: &EventRecord) -> Result<()> {
    tx.execute(
        "INSERT INTO event_records VALUES(?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            id.as_str(),
            record.start as i64,
            record.end as i64,
            event_fixture::class_number(record.class),
            record.seq.map(|s| s as i64),
            record.kind,
            record.body
        ],
    )?;
    Ok(())
}
/// The end of the task's last record, 0 without records. One indexed lookup, no recount.
fn last_end(tx: &Transaction<'_>, id: &TaskId) -> Result<u64> {
    let end: Option<i64> = tx
        .query_row(
            "SELECT end FROM event_records INDEXED BY event_starts WHERE task=?1 ORDER BY start DESC LIMIT 1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    end.map_or(Ok(0), |end| {
        u64::try_from(end).map_err(|_| Error::Incompatible)
    })
}
/// The seq of the task's last valid record, read backwards along the start index. It walks
/// back over any trailing non-valid records, so it is one lookup for a log ending in a valid
/// event and, at worst, every record of the task when none is valid (an all-oversized log).
fn last_valid_seq(tx: &Transaction<'_>, id: &TaskId) -> Result<Option<u64>> {
    let seq: Option<Option<i64>> = tx
        .query_row(
            "SELECT seq FROM event_records INDEXED BY event_starts WHERE task=?1 AND class=0 ORDER BY start DESC LIMIT 1",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    match seq {
        None => Ok(None),
        Some(Some(seq)) if seq >= 0 => Ok(Some(seq as u64)),
        Some(_) => Err(Error::Incompatible),
    }
}
