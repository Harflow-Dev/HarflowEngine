#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Shared helpers for the production event writer tests: appends built from the fixture event
//! bodies, new-stream facts, and the stored state an append may change.

use crate::fixture_support::{World, event_body, later, one, read, stream, stream_generation, tid};
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_storage::EventAppend;

/// A valid event line whose raw line is exactly its stored body.
pub fn ev(seq: u64, kind: &str) -> EventAppend {
    let body = event_body(seq, kind);
    EventAppend {
        seq,
        kind: kind.into(),
        raw_len: body.len() as u64,
        body,
    }
}

/// A line over the raw limit, carrying values the writer must neither check nor store.
pub fn oversized(raw_len: u64) -> EventAppend {
    EventAppend {
        seq: 0,
        kind: "k".repeat(300),
        raw_len,
        body: vec![0xff; 20_000],
    }
}

/// Facts that differ from the fixture task's in every column but the EOF seq, which a stream
/// may carry only when it ends with that `task_finished` record.
pub fn live_facts() -> LogFacts {
    LogFacts {
        task_status: Some("running".into()),
        available: true,
        capture: CaptureState::Live,
        output_drained: Some(false),
        eof_finished_seq: None,
        within_grace: true,
    }
}

/// The facts the fixture wrapper derives from a completed, drained fixture task.
pub fn terminal_facts() -> LogFacts {
    LogFacts {
        task_status: Some("completed".into()),
        available: true,
        capture: CaptureState::ObservedTerminal,
        output_drained: Some(true),
        eof_finished_seq: None,
        within_grace: false,
    }
}

pub fn append(w: &World, id: &str, events: &[EventAppend]) -> hf_storage::Result<()> {
    w.store
        .append_events(&tid(id), events, &live_facts(), later(5))
}

pub fn put(w: &World, id: &str, events: &[EventAppend]) {
    append(w, id, events).unwrap();
}

/// `start|end|class|seq|kind|body` of every record of the task with the body bytes quoted,
/// in physical order.
pub fn event_rows(w: &World, id: &str) -> Vec<String> {
    let db = w.db();
    let mut statement = db
        .prepare("SELECT quote(start)||'|'||quote(end)||'|'||quote(class)||'|'||quote(seq)||'|'||quote(kind)||'|'||quote(body) FROM event_records WHERE task=?1 ORDER BY start")
        .unwrap();
    statement
        .query_map([id], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

pub fn stream_count(w: &World, id: &str) -> i64 {
    crate::fixture_support::count(&w.db(), "event_streams", "task", id)
}

/// Everything about the task's log: the stream row and generation (`NONE` without a stream)
/// and every record.
pub fn log(w: &World, id: &str) -> (String, String, Vec<String>) {
    if stream_count(w, id) == 0 {
        return ("NONE".into(), "NONE".into(), event_rows(w, id));
    }
    (stream(w, id), stream_generation(w, id), event_rows(w, id))
}

pub fn complete_end(w: &World, id: &str) -> i64 {
    one(
        &w.db(),
        "SELECT complete_end FROM event_streams WHERE task=?1",
        [id],
    )
}

/// The seqs of every valid event, paged oldest first through the newer cursors.
pub fn all_seqs(w: &World, id: &str) -> Vec<u64> {
    let mut page = read(w, id, true, None).unwrap();
    let mut seqs: Vec<u64> = page.events.iter().filter_map(|e| e.seq).collect();
    while page.has_newer {
        page = read(w, id, false, page.newer_cursor.clone()).unwrap();
        seqs.extend(page.events.iter().filter_map(|e| e.seq));
    }
    seqs
}
