use crate::{Error, Result, Store, connection};
use hf_application::event_read::{EventReadError, EventReadPort, EventReadResult};
use hf_domain::{
    event_read::{CaptureState, EventError, EventQuery, LogFacts},
    task_read::{AuthorizedScope, Lookup, TaskId},
};
use rusqlite::{Connection, OptionalExtension};
use std::time::Instant;
pub struct EventReadSession<'a> {
    store: &'a Store,
}
impl<'a> EventReadSession<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }
}
impl EventReadPort for EventReadSession<'_> {
    fn execute(
        &self,
        query: &EventQuery,
        deadline: Instant,
    ) -> std::result::Result<EventReadResult, EventReadError> {
        query.validate().map_err(EventReadError::InvalidQuery)?;
        self.store
            .event_read(query, deadline)
            .map_err(|error| match error {
                Error::EventReadUnavailable | Error::TaskReadUnavailable => {
                    EventReadError::Unavailable
                }
                Error::InvalidEventQuery => EventReadError::InvalidQuery(EventError::InvalidCursor),
                Error::ExpiredEventCursor => {
                    EventReadError::InvalidQuery(EventError::ExpiredCursor)
                }
                _ => EventReadError::Storage,
            })
    }
}
impl Store {
    pub fn event_read(&self, query: &EventQuery, deadline: Instant) -> Result<EventReadResult> {
        query.validate().map_err(|_| Error::InvalidEventQuery)?;
        let mut db = self.admit(deadline)?;
        let tx = db.transaction()?;
        connection::timeout(&tx, deadline)?;
        let (_, coverage) =
            crate::task_read::dataset_optional(&tx)?.ok_or(Error::TaskReadUnavailable)?;
        let lookup = crate::task_query::lookup(
            &tx,
            &AuthorizedScope::Global,
            &query.task,
            coverage,
            deadline,
        )?;
        let mut result = empty(lookup.clone());
        if let Lookup::Found(task) = lookup {
            result.task = Some(task.clone());
            query
                .validate_cursor(&task)
                .map_err(|_| Error::InvalidEventQuery)?;
            crate::event_fixture::require_schema(&tx)?;
            let stream = stream_optional(&tx, &task)?;
            #[cfg(all(test, unix))]
            crate::tests::stage("event_read_snapshot");
            if let Some(stream) = stream {
                validate_cursor(self, query, &task, &stream)?;
                result.facts = Some(stream.facts.clone());
                if stream.facts.available {
                    crate::event_scan::read(
                        self,
                        &tx,
                        query,
                        &task,
                        &stream,
                        &mut result,
                        deadline,
                    )?;
                } else if query.cursor.is_some() {
                    return Err(Error::ExpiredEventCursor);
                }
            } else if query.cursor.is_some() {
                return Err(Error::ExpiredEventCursor);
            } else {
                result.facts = Some(LogFacts {
                    task_status: None,
                    available: false,
                    capture: CaptureState::Unknown,
                    output_drained: None,
                    eof_finished_seq: None,
                    within_grace: false,
                });
            }
        }
        self.binding.check()?;
        connection::check_deadline(deadline)?;
        Ok(result)
    }
}
pub(crate) struct Stream {
    pub generation: String,
    pub complete_end: u64,
    pub partial_tail: u64,
    pub facts: LogFacts,
}
impl Stream {
    pub fn size(&self) -> u64 {
        self.complete_end + self.partial_tail
    }
}
pub(crate) fn stream_optional(db: &Connection, task: &TaskId) -> Result<Option<Stream>> {
    type RawStream = (
        String,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        Option<String>,
    );
    let raw: Option<RawStream> = db.query_row(
        "SELECT CASE WHEN typeof(generation)='text' THEN CAST(substr(CAST(generation AS BLOB),1,33) AS TEXT) END,complete_end,partial_tail,available,capture,drained,eof_seq,grace,CASE WHEN typeof(task_status)='text' THEN CAST(substr(CAST(task_status AS BLOB),1,257) AS TEXT) WHEN task_status IS NULL THEN NULL ELSE 1 END FROM event_streams WHERE task=?1",[task.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional()?;
    let Some((generation, end, tail, available, capture, drained, eof, grace, task_status)) = raw
    else {
        return Ok(None);
    };
    if generation.len() != 32
        || !generation.bytes().all(|b| b.is_ascii_hexdigit())
        || end < 0
        || tail < 0
        || end.checked_add(tail).is_none()
        || eof.is_some_and(|s| s < 0)
    {
        return Err(Error::EventReadUnavailable);
    }
    let capture = match capture {
        0 => CaptureState::Live,
        1 => CaptureState::LostWithOwner,
        2 => CaptureState::ObservedTerminal,
        3 => CaptureState::Unknown,
        _ => return Err(Error::EventReadUnavailable),
    };
    let mut stream = Stream {
        generation,
        complete_end: end as u64,
        partial_tail: tail as u64,
        facts: LogFacts {
            task_status,
            available: boolean(available)?,
            capture,
            output_drained: drained.map(boolean).transpose()?,
            eof_finished_seq: eof.map(|s| s as u64),
            within_grace: boolean(grace)?,
        },
    };
    if stream
        .facts
        .task_status
        .as_ref()
        .is_some_and(|s| s.len() > 256)
    {
        return Err(Error::EventReadUnavailable);
    }
    validate_final(db, task, &mut stream)?;
    Ok(Some(stream))
}
fn boolean(value: i64) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::EventReadUnavailable),
    }
}
pub(crate) fn validate_final(db: &Connection, task: &TaskId, stream: &mut Stream) -> Result<()> {
    let seq = if stream.partial_tail == 0 {
        let tail:Option<(i64,i64,Vec<u8>)>=db.query_row("SELECT start,seq,CASE WHEN typeof(body)='blob' AND length(body)<=16384 THEN body END FROM event_records INDEXED BY event_ends WHERE task=?1 AND end=?2 AND class=0 AND kind='task_finished' AND start>=0 AND end>start AND end-start<=1000001 AND typeof(seq)='integer' AND seq>=0",rusqlite::params![task.as_str(),stream.complete_end as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((start, seq, body)) = tail {
            let record = hf_application::event_read::EventRecord {
                start: start as u64,
                end: stream.complete_end,
                class: hf_application::event_read::RecordClass::Valid,
                seq: Some(seq as u64),
                kind: Some(String::from("task_finished")),
                body,
            };
            crate::event_fixture::validate_record(&record)?;
            Some(seq as u64)
        } else {
            None
        }
    } else {
        None
    };
    if stream.facts.eof_finished_seq.is_some() && stream.facts.eof_finished_seq != seq {
        return Err(Error::EventReadUnavailable);
    }
    stream.facts.eof_finished_seq = seq;
    Ok(())
}

fn validate_cursor(
    store: &Store,
    query: &EventQuery,
    task: &TaskId,
    stream: &Stream,
) -> Result<()> {
    if let Some(cursor) = &query.cursor {
        if cursor.task != *task {
            return Err(Error::InvalidEventQuery);
        }
        if cursor.store != store.identity.generation
            || cursor.generation != stream.generation
            || cursor.boundary > stream.size()
        {
            return Err(Error::ExpiredEventCursor);
        }
    }
    Ok(())
}
fn empty(lookup: Lookup) -> EventReadResult {
    EventReadResult {
        task: None,
        lookup,
        events: Vec::new(),
        older_cursor: None,
        newer_cursor: None,
        has_older: false,
        has_newer: false,
        span: None,
        malformed: 0,
        oversized: 0,
        gaps: Vec::new(),
        facts: None,
        decoded: 0,
        scanned_bytes: 0,
    }
}
