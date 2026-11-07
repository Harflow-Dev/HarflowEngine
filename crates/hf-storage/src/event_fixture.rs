use crate::{Error, Result, Store, connection};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::{
    event_read::{CaptureState, LogFacts},
    task_read::TaskId,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::time::Instant;
pub(crate) const EVENT_SCHEMA: &str = "CREATE TABLE event_dataset(singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL); INSERT INTO event_dataset VALUES(1,1); CREATE TABLE event_streams(task TEXT PRIMARY KEY,generation TEXT NOT NULL,complete_end INTEGER NOT NULL,partial_tail INTEGER NOT NULL,available INTEGER NOT NULL,capture INTEGER NOT NULL,drained INTEGER,eof_seq INTEGER,grace INTEGER NOT NULL,task_status TEXT); CREATE TABLE event_records(task TEXT NOT NULL,start INTEGER NOT NULL,end INTEGER NOT NULL,class INTEGER NOT NULL,seq INTEGER,kind TEXT,body BLOB NOT NULL,PRIMARY KEY(task,start)); CREATE INDEX event_starts ON event_records(task,start); CREATE INDEX event_ends ON event_records(task,end);";
pub struct EventFixture {
    pub task: TaskId,
    pub records: Vec<EventRecord>,
    pub partial_tail: u64,
    pub facts: LogFacts,
}
impl Store {
    pub fn replace_event_fixture(&self, fixture: &EventFixture, deadline: Instant) -> Result<()> {
        self.write_event_fixture(fixture, false, deadline)
    }
    /// Records begin at the previous complete end; partial_tail is the new tail length.
    pub fn append_event_fixture(&self, fixture: &EventFixture, deadline: Instant) -> Result<()> {
        self.write_event_fixture(fixture, true, deadline)
    }
    fn write_event_fixture(
        &self,
        fixture: &EventFixture,
        append: bool,
        deadline: Instant,
    ) -> Result<()> {
        validate(self, fixture, deadline)?;
        let mut db = self.admit(deadline)?;
        let _io = self.binding.io(deadline)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        self.write_in(&tx, fixture, append, deadline)?;
        self.binding.check()?;
        connection::timeout(&tx, deadline)?;
        tx.commit().map_err(|_| Error::CommitOutcomeUnknown)?;
        self.binding
            .check()
            .map_err(|_| Error::CommitOutcomeUnknown)
    }
    /// The writes of one event fixture inside a caller-owned immediate transaction.
    pub(crate) fn write_in(
        &self,
        tx: &Connection,
        fixture: &EventFixture,
        append: bool,
        deadline: Instant,
    ) -> Result<()> {
        require_schema(tx)?;
        let previous = crate::event_read::stream_optional(tx, &fixture.task)?;
        let (old_count, old_bytes) =
            crate::event_checks::existing(self, tx, &fixture.task, previous.as_ref(), deadline)?;
        if append {
            self.check_bounds(
                old_count.saturating_add(fixture.records.len()),
                old_bytes.saturating_add(
                    fixture
                        .records
                        .iter()
                        .map(|r| r.body.len() + r.kind.as_ref().map_or(0, String::len) + 64)
                        .sum::<usize>(),
                ),
            )?;
        }
        let (end, generation) = prepare_stream(tx, fixture, append, previous.as_ref())?;
        connection::timeout(tx, deadline)?;
        if !append {
            tx.execute(
                "DELETE FROM event_records WHERE task=?1",
                [fixture.task.as_str()],
            )?;
        }
        write_stream(tx, fixture, end, &generation)?;
        for record in &fixture.records {
            connection::timeout(tx, deadline)?;
            tx.execute(
                "INSERT INTO event_records VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![
                    fixture.task.as_str(),
                    record.start as i64,
                    record.end as i64,
                    class_number(record.class),
                    record.seq.map(|s| s as i64),
                    record.kind,
                    record.body
                ],
            )?;
        }
        crate::event_read::stream_optional(tx, &fixture.task)?;
        Ok(())
    }
}
fn prepare_stream(
    tx: &Connection,
    fixture: &EventFixture,
    append: bool,
    previous: Option<&crate::event_read::Stream>,
) -> Result<(u64, String)> {
    let begin = if append {
        previous.ok_or(Error::EventReadUnavailable)?.complete_end
    } else {
        0
    };
    if fixture.records.first().is_some_and(|r| r.start != begin) {
        return Err(Error::InvalidEventQuery);
    }
    let end = fixture.records.last().map_or(begin, |r| r.end);
    if end
        .checked_add(fixture.partial_tail)
        .is_none_or(|size| size > i64::MAX as u64)
    {
        return Err(Error::LimitExceeded);
    }
    // Appends preserve every previously observed partial byte logically. Completion
    // must cover that tail; fixtures provide no byte content proof for partials.
    if append
        && previous.is_some_and(|old| {
            end + fixture.partial_tail < old.size()
                || (!fixture.records.is_empty() && fixture.records[0].end < old.size())
        })
    {
        return Err(Error::InvalidEventQuery);
    }
    let preserve =
        append && previous.is_some_and(|old| old.facts.available && fixture.facts.available);
    let generation: String = if preserve {
        previous.ok_or(Error::Incompatible)?.generation.clone()
    } else {
        tx.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?
    };
    Ok((end, generation))
}

pub(crate) fn require_schema(db: &Connection) -> Result<()> {
    let tables: i64 = db.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('event_dataset','event_streams','event_records')", [], |r| r.get(0))?;
    if tables != 3 {
        return Err(Error::EventReadUnavailable);
    }
    let version: Option<i64> = db
        .query_row(
            "SELECT version FROM event_dataset WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if version != Some(1) {
        return Err(Error::EventReadUnavailable);
    }
    Ok(())
}
fn write_stream(db: &Connection, fixture: &EventFixture, end: u64, generation: &str) -> Result<()> {
    let capture = match fixture.facts.capture {
        CaptureState::Live => 0,
        CaptureState::LostWithOwner => 1,
        CaptureState::ObservedTerminal => 2,
        CaptureState::Unknown => 3,
    };
    db.execute("INSERT INTO event_streams VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(task) DO UPDATE SET generation=excluded.generation,complete_end=excluded.complete_end,partial_tail=excluded.partial_tail,available=excluded.available,capture=excluded.capture,drained=excluded.drained,eof_seq=excluded.eof_seq,grace=excluded.grace,task_status=excluded.task_status", rusqlite::params![fixture.task.as_str(), generation,end as i64,fixture.partial_tail as i64,fixture.facts.available,capture,fixture.facts.output_drained,fixture.facts.eof_finished_seq.map(|s|s as i64),fixture.facts.within_grace,fixture.facts.task_status])?;
    Ok(())
}
pub(crate) fn class_number(class: RecordClass) -> i64 {
    match class {
        RecordClass::Valid => 0,
        RecordClass::Blank => 1,
        RecordClass::Malformed => 2,
        RecordClass::Oversized => 3,
    }
}
pub(crate) fn validate(store: &Store, fixture: &EventFixture, deadline: Instant) -> Result<()> {
    let mut previous = fixture.records.first().map_or(0, |r| r.start);
    let mut bytes = 512usize;
    for record in &fixture.records {
        connection::check_deadline(deadline)?;
        validate_record(record)?;
        if record.start != previous {
            return Err(Error::InvalidEventQuery);
        }
        previous = record.end;
        bytes = bytes
            .checked_add(record.body.len() + record.kind.as_ref().map_or(0, String::len) + 64)
            .ok_or(Error::LimitExceeded)?;
    }
    if fixture
        .facts
        .task_status
        .as_ref()
        .is_some_and(|s| s.len() > 256)
    {
        return Err(Error::InvalidEventQuery);
    }
    if fixture
        .facts
        .eof_finished_seq
        .is_some_and(|seq| seq > i64::MAX as u64)
    {
        return Err(Error::InvalidEventQuery);
    }
    store.check_bounds(fixture.records.len(), bytes)
}
pub(crate) fn validate_record(record: &EventRecord) -> Result<()> {
    if record.start >= record.end || record.end > i64::MAX as u64 || record.body.len() > 16384 {
        return Err(Error::InvalidEventQuery);
    }
    if record.class == RecordClass::Valid {
        if record.seq.is_none_or(|seq| seq > i64::MAX as u64)
            || record.kind.as_ref().is_none_or(|kind| kind.len() > 128)
            || std::str::from_utf8(&record.body).is_err()
            || record.end - record.start > 1_000_001
        {
            return Err(Error::InvalidEventQuery);
        }
    } else if record.seq.is_some() || record.kind.is_some() || !record.body.is_empty() {
        return Err(Error::InvalidEventQuery);
    }
    if (record.class == RecordClass::Oversized && record.end - record.start <= 1_000_001)
        || (record.class == RecordClass::Blank && record.end - record.start != 1)
        || (record.class == RecordClass::Malformed && record.end - record.start > 1_000_001)
    {
        return Err(Error::InvalidEventQuery);
    }
    Ok(())
}
