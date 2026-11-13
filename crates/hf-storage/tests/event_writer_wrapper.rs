#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The fixture wrapper over the production event append: the fixture's own stream behaviour
//! (facts refreshed from the task, generation rotated on an unavailable stream) and the
//! dropped cumulative row cap.
mod common;
mod event_writer_support;
mod fixture_support;
mod task_writer_support;
use event_writer_support::{all_seqs, ev, event_rows, live_facts, log, terminal_facts};
use fixture_support::{
    A, World, done, event, event_body, is_hex32, later, stream, stream_generation, tid, world,
};
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::event_read::LogFacts;
use hf_storage::{Error, EventFixture};
use task_writer_support::published;

/// A valid record of `event(seq, kind)` starting at `start`.
fn record(start: u64, seq: u64, kind: &str) -> EventRecord {
    let body = event_body(seq, kind);
    EventRecord {
        start,
        end: start + body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some(kind.into()),
        body,
    }
}

/// Task A with a stream of `task_started` 0 and `task_finished` 1 stored with `facts`.
fn replaced(facts: LogFacts) -> World {
    let w = world();
    w.add(&done(A));
    let first = record(0, 0, "task_started");
    let last = record(first.end, 1, "task_finished");
    w.store
        .replace_event_fixture(
            &EventFixture {
                task: tid(A),
                records: vec![first, last],
                partial_tail: 0,
                facts,
            },
            later(5),
        )
        .unwrap();
    w
}

#[test]
fn the_wrapper_appends_to_a_closed_stream_and_clears_its_eof_seq_where_production_refuses() {
    let mut closed = terminal_facts();
    closed.eof_finished_seq = Some(1);
    let w = replaced(closed);
    assert!(
        stream(&w, A).ends_with("|1|2|1|1|0|'completed'"),
        "{}",
        stream(&w, A)
    );
    let before = (published(&w), log(&w, A));
    let error = w
        .store
        .append_events(&tid(A), &[ev(2, "notice")], &live_facts(), later(5))
        .unwrap_err();
    assert!(matches!(error, Error::EventStreamClosed), "{error:?}");
    assert_eq!((published(&w), log(&w, A)), before);
    let generation = stream_generation(&w, A);
    w.append(A, &event(2, "notice"));
    let end = event_rows(&w, A).len();
    assert_eq!(end, 3);
    let complete: i64 = fixture_support::one(
        &w.db(),
        "SELECT max(end) FROM event_records WHERE task=?1",
        [A],
    );
    assert_eq!(
        stream(&w, A),
        format!("{complete}|0|1|2|1|NULL|0|'completed'"),
        "the task's facts replace the stored ones, EOF seq included"
    );
    assert_eq!(stream_generation(&w, A), generation);
    assert_eq!(all_seqs(&w, A), [0, 1, 2]);
}

#[test]
fn the_wrapper_rotates_the_generation_of_an_unavailable_stream_and_keeps_an_available_one() {
    let mut unavailable = terminal_facts();
    unavailable.available = false;
    let w = replaced(unavailable);
    let stored = stream_generation(&w, A);
    w.append(A, &event(2, "notice"));
    let rotated = stream_generation(&w, A);
    assert!(is_hex32(&rotated), "{rotated}");
    assert_ne!(rotated, stored);
    assert!(
        stream(&w, A).contains("|0|1|2|1|NULL|0|"),
        "{}",
        stream(&w, A)
    );
    w.append(A, &event(3, "tool_call"));
    assert_eq!(stream_generation(&w, A), rotated);

    let w = replaced(terminal_facts());
    let stored = stream_generation(&w, A);
    w.append(A, &event(2, "notice"));
    w.append(A, &event(3, "tool_call"));
    assert_eq!(stream_generation(&w, A), stored);
}

/// The default `Limits` allow 128 rows; the fixture append no longer recounts the whole log.
#[test]
fn fixture_appends_past_the_default_row_limit_succeed() {
    let w = world();
    w.add(&done(A));
    for seq in 0..140 {
        w.append(A, &event(seq, "assistant_text"));
    }
    assert_eq!(event_rows(&w, A).len(), 140);
    assert_eq!(all_seqs(&w, A), (0..140).collect::<Vec<u64>>());
}
