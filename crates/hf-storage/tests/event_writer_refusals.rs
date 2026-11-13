#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::append_events` refusals: every one leaves the log, the publication state and the
//! commit sequence as they were, including a bad record late in a batch.
mod common;
mod event_writer_support;
mod fixture_support;
mod task_writer_support;
use event_writer_support::{ev, live_facts, log, put, stream_count};
use fixture_support::{A, B, World, done, event, later, tid, world};
use hf_domain::event_read::LogFacts;
use hf_storage::{Error, EventAppend, FixtureLine};
use task_writer_support::published;

/// Appends with the given facts and returns the refusal, asserting nothing changed.
fn refused_with(w: &World, id: &str, events: &[EventAppend], facts: &LogFacts) -> Error {
    let before = (published(w), log(w, id));
    let error = w
        .store
        .append_events(&tid(id), events, facts, later(5))
        .unwrap_err();
    assert_eq!(
        (published(w), log(w, id)),
        before,
        "{error:?} wrote nothing"
    );
    error
}

fn refused(w: &World, id: &str, events: &[EventAppend]) -> Error {
    refused_with(w, id, events, &live_facts())
}

/// A world whose task A has a stream with seqs 0..=2.
fn stored() -> World {
    let w = world();
    w.add(&done(A));
    put(&w, A, &[ev(0, "a"), ev(1, "b"), ev(2, "c")]);
    w
}

#[test]
fn a_missing_task_is_refused_and_no_stream_is_created() {
    let w = world();
    let error = refused(&w, A, &[ev(0, "task_started")]);
    assert!(matches!(error, Error::TaskMissing), "{error:?}");
    assert_eq!(stream_count(&w, A), 0);
}

#[test]
fn a_damaged_task_is_refused_with_or_without_a_stream() {
    let w = stored();
    w.add(&done(B));
    for id in [A, B] {
        w.store.damage_task_fixture(&tid(id), later(5)).unwrap();
        let error = refused(&w, id, &[ev(3, "d")]);
        assert!(matches!(error, Error::TaskMissing), "{id}: {error:?}");
    }
    assert_eq!(stream_count(&w, B), 0);
}

#[test]
fn a_seq_not_above_the_stored_last_valid_seq_is_refused() {
    let w = stored();
    for seq in [0, 1, 2] {
        let error = refused(&w, A, &[ev(seq, "d")]);
        assert!(matches!(error, Error::EventOutOfOrder), "{seq}: {error:?}");
    }
    put(&w, A, &[ev(3, "d")]);
}

#[test]
fn a_duplicate_or_regressing_seq_within_a_batch_is_refused() {
    for batch in [
        vec![ev(0, "a"), ev(0, "b")],
        vec![ev(0, "a"), ev(2, "b"), ev(1, "c")],
    ] {
        let w = world();
        w.add(&done(A));
        let error = refused(&w, A, &batch);
        assert!(matches!(error, Error::EventOutOfOrder), "{error:?}");
        assert_eq!(
            stream_count(&w, A),
            0,
            "the stream creation rolled back too"
        );
    }
}

/// The out-of-order record is found inside the transaction after earlier rows were inserted
/// and `complete_end` would have moved; the span overflow is found there too.
#[test]
fn a_bad_record_late_in_a_batch_rolls_back_the_whole_batch() {
    let w = stored();
    let late_order: Vec<EventAppend> = (3..20).map(|s| ev(s, "d")).chain([ev(19, "x")]).collect();
    let error = refused(&w, A, &late_order);
    assert!(matches!(error, Error::EventOutOfOrder), "{error:?}");
    let mut span = ev(0, "x");
    span.raw_len = i64::MAX as u64;
    let error = refused(&w, A, &[ev(3, "d"), ev(4, "e"), span]);
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    let mut body = ev(5, "x");
    body.body = vec![b'a'; hf_storage::EVENT_BODY_LIMIT + 1];
    let error = refused(&w, A, &[ev(3, "d"), ev(4, "e"), body]);
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    put(&w, A, &[ev(3, "d")]);
}

#[test]
fn an_empty_line_or_a_body_that_is_not_utf8_is_an_invalid_query() {
    let w = stored();
    let mut empty = ev(3, "d");
    empty.raw_len = 0;
    let mut binary = ev(3, "d");
    binary.body = vec![b'{', 0xff, 0xfe, b'}'];
    for bad in [empty, binary] {
        let error = refused(&w, A, &[bad]);
        assert!(matches!(error, Error::InvalidEventQuery), "{error:?}");
    }
}

#[test]
fn a_seq_beyond_i64_is_refused_as_a_limit() {
    let w = stored();
    let error = refused(&w, A, &[ev(i64::MAX as u64 + 1, "d")]);
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
}

#[test]
fn new_stream_facts_over_their_bounds_are_refused_as_a_limit() {
    let w = world();
    w.add(&done(A));
    let mut status = live_facts();
    status.task_status = Some("s".repeat(257));
    let mut eof = live_facts();
    eof.eof_finished_seq = Some(i64::MAX as u64 + 1);
    for facts in [status, eof] {
        let error = refused_with(&w, A, &[ev(0, "a")], &facts);
        assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    }
    let mut fits = live_facts();
    fits.task_status = Some("s".repeat(256));
    w.store
        .append_events(&tid(A), &[ev(0, "a")], &fits, later(5))
        .unwrap();
    let mut ignored = live_facts();
    ignored.task_status = Some("s".repeat(257));
    ignored.eof_finished_seq = Some(i64::MAX as u64 + 1);
    w.store
        .append_events(&tid(A), &[ev(1, "b")], &ignored, later(5))
        .expect("an existing stream neither checks nor stores facts_if_new");
}

#[test]
fn a_stream_with_a_partial_tail_is_incompatible() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.append(A, &FixtureLine::Partial { len: 11 });
    let error = refused(&w, A, &[ev(1, "assistant_text")]);
    assert!(matches!(error, Error::Incompatible), "{error:?}");
    w.add(&done(B));
    w.append(B, &FixtureLine::Partial { len: 5 });
    let error = refused(&w, B, &[ev(0, "task_started")]);
    assert!(matches!(error, Error::Incompatible), "{error:?}");
}

#[test]
fn a_stream_whose_last_record_does_not_end_at_complete_end_is_incompatible() {
    let w = stored();
    w.raw()
        .execute(
            "UPDATE event_streams SET complete_end=complete_end+5 WHERE task=?1",
            [A],
        )
        .unwrap();
    let error = refused(&w, A, &[ev(3, "d")]);
    assert!(matches!(error, Error::Incompatible), "{error:?}");
}

/// A stream may carry an EOF seq only when its last record is that `task_finished` event, so
/// `facts_if_new` naming one lands only with a batch that ends there (`InvalidEventQuery`
/// otherwise), and a stream that stores one takes no further appends (`EventStreamClosed`).
/// Both are checked before any row is inserted.
#[test]
fn an_eof_seq_holds_only_for_a_stream_ending_in_that_task_finished_event() {
    let w = world();
    w.add(&done(A));
    let mut eof = live_facts();
    eof.eof_finished_seq = Some(7);
    let error = refused_with(&w, A, &[ev(0, "task_started"), ev(7, "tool_call")], &eof);
    assert!(matches!(error, Error::InvalidEventQuery), "{error:?}");
    let error = refused_with(
        &w,
        A,
        &[ev(0, "task_started"), ev(6, "task_finished")],
        &eof,
    );
    assert!(matches!(error, Error::InvalidEventQuery), "{error:?}");
    assert_eq!(stream_count(&w, A), 0);
    w.store
        .append_events(
            &tid(A),
            &[ev(0, "task_started"), ev(7, "task_finished")],
            &eof,
            later(5),
        )
        .unwrap();
    assert_eq!(
        stream_count(&w, A),
        1,
        "the closing batch lands on creation"
    );
    let error = refused(&w, A, &[ev(8, "late")]);
    assert!(matches!(error, Error::EventStreamClosed), "{error:?}");
}
