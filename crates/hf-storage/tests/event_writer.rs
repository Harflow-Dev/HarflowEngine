#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::append_events`: stream creation and preservation, record spans, the body, kind and
//! raw-line bounds, seq order, commit accounting and parity with the fixture wrapper.
mod common;
mod event_writer_support;
mod fixture_support;
mod task_writer_support;
use event_writer_support::{
    all_seqs, append, complete_end, ev, event_rows, live_facts, log, oversized, put, stream_count,
    terminal_facts,
};
use fixture_support::{
    A, World, done, event, event_len, hex, is_hex32, later, read, stream, stream_generation, tid,
    world,
};
use hf_application::event_read::EventReadResult;
use hf_domain::event_read::{EventCursor, EventFilter, EventQuery};
use hf_storage::{EVENT_BODY_LIMIT, EVENT_KIND_LIMIT, EVENT_LINE_LIMIT, Error, EventAppend};
use task_writer_support::published;

#[test]
fn the_first_batch_creates_the_stream_with_facts_if_new_and_contiguous_records() {
    let w = world();
    w.add(&done(A));
    let before = published(&w);
    put(&w, A, &[ev(0, "task_started"), ev(1, "assistant_text")]);
    let (first, second) = (event_len(0, "task_started"), event_len(1, "assistant_text"));
    assert_eq!(
        event_rows(&w, A),
        [
            format!(
                "0|{first}|0|0|'task_started'|{}",
                hex(&fixture_support::event_body(0, "task_started"))
            ),
            format!(
                "{first}|{}|0|1|'assistant_text'|{}",
                first + second,
                hex(&fixture_support::event_body(1, "assistant_text"))
            ),
        ]
    );
    assert_eq!(
        stream(&w, A),
        format!("{}|0|1|0|0|NULL|1|'running'", first + second)
    );
    assert!(is_hex32(&stream_generation(&w, A)));
    let after = published(&w);
    assert_eq!(
        (&after.0, after.1, after.2),
        (&before.0, before.1, before.2 + 1),
        "one commit, no dataset generation or watch revision change"
    );
}

#[test]
fn an_existing_stream_keeps_its_generation_and_facts_and_only_its_end_moves() {
    let w = world();
    w.add(&done(A));
    put(&w, A, &[ev(0, "task_started")]);
    let (generation, created) = (stream_generation(&w, A), stream(&w, A));
    let first = event_len(0, "task_started");
    w.store
        .append_events(&tid(A), &[ev(1, "tool_call")], &terminal_facts(), later(5))
        .unwrap();
    assert_eq!(stream_generation(&w, A), generation);
    let end = first + event_len(1, "tool_call");
    assert_eq!(stream(&w, A), format!("{end}|0|1|0|0|NULL|1|'running'"));
    assert_ne!(stream(&w, A), created, "only complete_end changed");
}

#[test]
fn each_batch_is_one_commit_without_revision_or_generation_change() {
    let w = world();
    w.add(&done(A));
    let before = published(&w);
    put(
        &w,
        A,
        &[ev(0, "task_started"), ev(1, "notice"), ev(2, "tool_call")],
    );
    w.store
        .append_event(&tid(A), &ev(3, "tool_result"), &live_facts(), later(5))
        .unwrap();
    put(&w, A, &[ev(4, "usage"), ev(5, "assistant_text")]);
    let after = published(&w);
    assert_eq!((&after.0, after.1), (&before.0, before.1));
    assert_eq!(after.2, before.2 + 3);
    assert_eq!(all_seqs(&w, A), [0, 1, 2, 3, 4, 5]);
}

#[test]
fn an_empty_batch_commits_nothing_even_for_a_missing_task() {
    let w = world();
    let before = published(&w);
    append(&w, A, &[]).unwrap();
    w.add(&done(A));
    let added = published(&w);
    append(&w, A, &[]).unwrap();
    assert_eq!(published(&w), added);
    assert_ne!(added, before);
    assert_eq!(stream_count(&w, A), 0, "no stream is created");
}

/// The default `Limits` cap fixture writes at 128 rows; the production append never recounts.
#[test]
fn appends_past_the_default_row_limit_land_one_by_one_and_in_one_batch() {
    let w = world();
    w.add(&done(A));
    for seq in 0..140 {
        w.store
            .append_event(&tid(A), &ev(seq, "assistant_text"), &live_facts(), later(5))
            .unwrap();
    }
    let batch: Vec<EventAppend> = (140..340).map(|s| ev(s, "tool_call")).collect();
    put(&w, A, &batch);
    assert_eq!(event_rows(&w, A).len(), 340);
    assert_eq!(all_seqs(&w, A), (0..340).collect::<Vec<u64>>());
}

#[test]
fn a_body_of_16384_bytes_lands_and_16385_is_refused() {
    let w = world();
    w.add(&done(A));
    let mut fits = ev(0, "assistant_text");
    fits.body = vec![b'a'; EVENT_BODY_LIMIT];
    fits.raw_len = 20_000;
    put(&w, A, &[fits]);
    assert_eq!(complete_end(&w, A), 20_001);
    let stored: Vec<u8> =
        fixture_support::one(&w.db(), "SELECT body FROM event_records WHERE task=?1", [A]);
    assert_eq!(stored.len(), 16_384);
    let mut over = ev(1, "assistant_text");
    over.body = vec![b'a'; EVENT_BODY_LIMIT + 1];
    over.raw_len = 20_000;
    let kept = log(&w, A);
    assert!(matches!(append(&w, A, &[over]), Err(Error::LimitExceeded)));
    assert_eq!(log(&w, A), kept);
}

#[test]
fn a_kind_of_128_bytes_lands_and_129_is_refused() {
    let w = world();
    w.add(&done(A));
    put(&w, A, &[ev(0, &"k".repeat(EVENT_KIND_LIMIT))]);
    let kept = log(&w, A);
    let error = append(&w, A, &[ev(1, &"k".repeat(EVENT_KIND_LIMIT + 1))]).unwrap_err();
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    assert_eq!(log(&w, A), kept);
}

#[test]
fn a_raw_line_of_1000000_bytes_is_valid_and_1000001_is_oversized() {
    let w = world();
    w.add(&done(A));
    let mut valid = ev(3, "assistant_text");
    valid.raw_len = EVENT_LINE_LIMIT;
    put(&w, A, &[valid]);
    put(&w, A, &[oversized(EVENT_LINE_LIMIT + 1)]);
    let rows = event_rows(&w, A);
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0].starts_with("0|1000001|0|3|'assistant_text'|"),
        "{}",
        rows[0]
    );
    assert_eq!(rows[1], "1000001|2000003|3|NULL|NULL|X''");
    assert_eq!(complete_end(&w, A), 2_000_003);
    // The oversized line's seq 0 neither counted nor advanced the order: 4 follows 3.
    put(&w, A, &[ev(4, "tool_call")]);
    assert_eq!(all_seqs(&w, A), [3, 4]);
    let error = append(&w, A, &[ev(4, "tool_call")]).unwrap_err();
    assert!(matches!(error, Error::EventOutOfOrder), "{error:?}");
}

#[test]
fn an_oversized_line_between_valid_ones_does_not_reset_the_last_valid_seq() {
    let w = world();
    w.add(&done(A));
    put(&w, A, &[ev(5, "a"), oversized(2_000_000)]);
    let kept = log(&w, A);
    for seq in [0, 5] {
        let error = append(&w, A, &[ev(seq, "b")]).unwrap_err();
        assert!(matches!(error, Error::EventOutOfOrder), "{seq}: {error:?}");
        assert_eq!(log(&w, A), kept);
    }
    put(&w, A, &[ev(6, "b")]);
}

#[test]
fn seqs_must_increase_but_may_skip() {
    let w = world();
    w.add(&done(A));
    put(&w, A, &[ev(0, "task_started"), ev(10, "notice")]);
    put(&w, A, &[ev(11, "tool_call"), ev(i64::MAX as u64, "usage")]);
    assert_eq!(all_seqs(&w, A), [0, 10, 11, i64::MAX as u64]);
}

#[test]
fn generation_and_read_cursors_survive_appends() {
    let w = world();
    w.add(&done(A));
    let kinds = ["task_started", "notice", "tool_call", "tool_result"];
    let first: Vec<EventAppend> = (0..4).map(|s| ev(s, kinds[s as usize])).collect();
    put(&w, A, &first);
    let generation = stream_generation(&w, A);
    let newer = page(&w, true, None).newer_cursor.unwrap();
    let older = page(&w, false, None).older_cursor.unwrap();
    put(&w, A, &[ev(4, "usage"), ev(5, "assistant_text")]);
    w.store
        .append_event(&tid(A), &ev(6, "notice"), &live_facts(), later(5))
        .unwrap();
    assert_eq!(stream_generation(&w, A), generation);
    assert_eq!(seqs(&page(&w, false, Some(newer))), [2, 3]);
    assert_eq!(seqs(&page(&w, false, Some(older))), [0, 1]);
    assert_eq!(all_seqs(&w, A), [0, 1, 2, 3, 4, 5, 6]);
}

/// A two-event page of task A's log.
fn page(w: &World, from_oldest: bool, cursor: Option<EventCursor>) -> EventReadResult {
    w.session()
        .event_read(
            &EventQuery {
                task: tid(A),
                limit: 2,
                filter: EventFilter::Default,
                from_oldest,
                cursor,
            },
            later(5),
        )
        .unwrap()
}

fn seqs(result: &EventReadResult) -> Vec<u64> {
    result.events.iter().filter_map(|e| e.seq).collect()
}

#[test]
fn the_fixture_wrapper_and_the_production_append_store_identical_rows() {
    let (production, wrapped) = (world(), world());
    let lines = [(0, "task_started"), (1, "assistant_text"), (7, "tool_call")];
    for w in [&production, &wrapped] {
        w.add(&done(A));
    }
    for (seq, kind) in lines {
        wrapped.append(A, &event(seq, kind));
        production
            .store
            .append_event(&tid(A), &ev(seq, kind), &terminal_facts(), later(5))
            .unwrap();
    }
    assert_eq!(event_rows(&production, A), event_rows(&wrapped, A));
    assert_eq!(stream(&production, A), stream(&wrapped, A));
    assert_eq!(
        read(&production, A, true, None).unwrap().events,
        read(&wrapped, A, true, None).unwrap().events
    );
    let error = wrapped
        .store
        .append_event_record_fixture(&tid(A), &event(7, "again"), later(5))
        .unwrap_err();
    assert!(matches!(error, Error::EventOutOfOrder), "{error:?}");
}
