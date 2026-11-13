//! Event-log fixture writers: one physical line at a time, with the Python port's
//! concatenation rules for an unterminated tail.
use crate::{
    Error, Result, Store, event_fixture, event_read,
    event_writer::{self, EventAppend},
};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::{
    event_read::{CaptureState, LogFacts},
    task_read::TaskId,
};
use rusqlite::{OptionalExtension, Transaction};
use std::time::Instant;
const LINE_LIMIT: u64 = 1_000_001;
const TERMINAL: [&str; 4] = ["completed", "failed", "timed_out", "cancelled"];
/// One physical line. `len` counts every byte of the line, including a newline when
/// the line is terminated. A `Partial` has no newline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixtureLine {
    Event {
        seq: u64,
        kind: String,
        body: Vec<u8>,
    },
    Malformed {
        len: u64,
    },
    Partial {
        len: u64,
    },
}
impl Store {
    /// Appends one line to the task's log, creating the stream when there is none. Stream
    /// facts are copied from the task's facts; the stream generation survives an append.
    /// An unterminated tail joins the next terminated line, which is then classified:
    /// the port only writes tails that cannot parse, so the joined line never is valid.
    pub fn append_event_record_fixture(
        &self,
        id: &TaskId,
        line: &FixtureLine,
        deadline: Instant,
    ) -> Result<()> {
        self.fixture_write(deadline, |tx| {
            event_fixture::require_schema(tx)?;
            let facts = log_facts(tx, id)?;
            let previous = event_read::stream_optional(tx, id)?;
            let fixture = build(id, line, previous.as_ref(), facts);
            event_fixture::validate(self, &fixture, deadline)?;
            match line {
                FixtureLine::Event { seq, kind, body }
                    if previous.as_ref().is_none_or(|s| s.partial_tail == 0) =>
                {
                    let event = EventAppend {
                        seq: *seq,
                        kind: kind.clone(),
                        raw_len: body.len() as u64,
                        body: body.clone(),
                    };
                    append_event(tx, &fixture, &event, previous.as_ref(), deadline)
                }
                _ => self.write_in(tx, &fixture, previous.is_some(), deadline),
            }
        })
    }
    /// Keeps the first `keep` physical lines, an unterminated final line included, and
    /// rotates the stream generation so that existing cursors expire.
    pub fn truncate_event_log_fixture(
        &self,
        id: &TaskId,
        keep: u64,
        deadline: Instant,
    ) -> Result<()> {
        self.fixture_write(deadline, |tx| {
            event_fixture::require_schema(tx)?;
            let stream = event_read::stream_optional(tx, id)?.ok_or(Error::FixtureMissing)?;
            let keep = i64::try_from(keep).unwrap_or(i64::MAX);
            let count: i64 = tx.query_row(
                "SELECT count(*) FROM event_records WHERE task=?1",
                [id.as_str()],
                |r| r.get(0),
            )?;
            let size = |value: u64| i64::try_from(value).map_err(|_| Error::Incompatible);
            let (end, tail) = if keep > count {
                (size(stream.complete_end)?, size(stream.partial_tail)?)
            } else if keep == count {
                (size(stream.complete_end)?, 0)
            } else if keep == 0 {
                (0, 0)
            } else {
                (nth_end(tx, id, keep)?, 0)
            };
            tx.execute(
                "DELETE FROM event_records WHERE task=?1 AND start>=?2",
                rusqlite::params![id.as_str(), end],
            )?;
            tx.execute(
                "UPDATE event_streams SET generation=lower(hex(randomblob(16))),complete_end=?2,partial_tail=?3 WHERE task=?1",
                rusqlite::params![id.as_str(), end, tail],
            )?;
            event_read::stream_optional(tx, id)?;
            Ok(())
        })
    }
    /// Removes the stream and its records; the task then has no log at all.
    pub fn remove_event_log_fixture(&self, id: &TaskId, deadline: Instant) -> Result<()> {
        self.fixture_write(deadline, |tx| {
            event_fixture::require_schema(tx)?;
            if event_read::stream_optional(tx, id)?.is_none() {
                return Err(Error::FixtureMissing);
            }
            tx.execute("DELETE FROM event_records WHERE task=?1", [id.as_str()])?;
            tx.execute("DELETE FROM event_streams WHERE task=?1", [id.as_str()])?;
            Ok(())
        })
    }
}
/// A terminated event after a complete line goes through the production append. The fixture
/// keeps its own stream semantics in front of it: the facts are refreshed from the task on
/// every append (so a stored EOF seq is cleared, as the fixture always did), and appending to
/// an unavailable stream rotates its generation. The append then keeps both.
fn append_event(
    tx: &Transaction<'_>,
    fixture: &event_fixture::EventFixture,
    event: &EventAppend,
    previous: Option<&event_read::Stream>,
    deadline: Instant,
) -> Result<()> {
    if let Some(previous) = previous {
        let generation: String = if previous.facts.available {
            previous.generation.clone()
        } else {
            tx.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?
        };
        event_fixture::write_stream(
            tx,
            &fixture.task,
            &fixture.facts,
            previous.complete_end,
            0,
            &generation,
        )?;
    }
    event_writer::append_in(
        tx,
        &fixture.task,
        std::slice::from_ref(event),
        &fixture.facts,
        deadline,
    )
    .map_err(|error| match error {
        Error::TaskMissing => Error::FixtureMissing,
        other => other,
    })?;
    Ok(())
}
fn nth_end(tx: &Transaction<'_>, id: &TaskId, keep: i64) -> Result<i64> {
    Ok(tx.query_row(
        "SELECT end FROM event_records WHERE task=?1 ORDER BY start LIMIT 1 OFFSET ?2",
        rusqlite::params![id.as_str(), keep - 1],
        |r| r.get(0),
    )?)
}
type FactsRow = (bool, Option<String>, Option<i64>, Option<bool>);
fn log_facts(tx: &Transaction<'_>, id: &TaskId) -> Result<LogFacts> {
    let row: Option<FactsRow> = tx
        .query_row(
            "SELECT valid,status,exit_code,output_drained FROM task_facts WHERE id=?1",
            [id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (valid, status, exit_code, drained) = row.ok_or(Error::FixtureMissing)?;
    if !valid {
        return Err(Error::FixtureMissing);
    }
    let observed = exit_code.is_some() && status.as_deref().is_some_and(|s| TERMINAL.contains(&s));
    Ok(LogFacts {
        task_status: status,
        available: true,
        capture: if observed {
            CaptureState::ObservedTerminal
        } else {
            CaptureState::Unknown
        },
        output_drained: drained,
        eof_finished_seq: None,
        within_grace: false,
    })
}
fn build(
    id: &TaskId,
    line: &FixtureLine,
    previous: Option<&event_read::Stream>,
    facts: LogFacts,
) -> event_fixture::EventFixture {
    let begin = previous.map_or(0, |s| s.complete_end);
    let tail = previous.map_or(0, |s| s.partial_tail);
    let (records, partial_tail) = match line {
        FixtureLine::Partial { len } => (Vec::new(), tail.saturating_add(*len)),
        FixtureLine::Event { seq, kind, body } if tail == 0 => {
            let end = begin.saturating_add(body.len() as u64 + 1);
            let record = EventRecord {
                start: begin,
                end,
                class: RecordClass::Valid,
                seq: Some(*seq),
                kind: Some(kind.clone()),
                body: body.clone(),
            };
            (vec![record], 0)
        }
        FixtureLine::Event { body, .. } => (
            vec![joined(begin, tail.saturating_add(body.len() as u64 + 1))],
            0,
        ),
        FixtureLine::Malformed { len } => (vec![joined(begin, tail.saturating_add(*len))], 0),
    };
    event_fixture::EventFixture {
        task: id.clone(),
        records,
        partial_tail,
        facts,
    }
}
/// A terminated line that is not a valid event, `total` bytes long.
fn joined(begin: u64, total: u64) -> EventRecord {
    let class = if total == 1 {
        RecordClass::Blank
    } else if total > LINE_LIMIT {
        RecordClass::Oversized
    } else {
        RecordClass::Malformed
    };
    EventRecord {
        start: begin,
        end: begin.saturating_add(total),
        class,
        seq: None,
        kind: None,
        body: Vec::new(),
    }
}
