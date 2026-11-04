#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common/event_fixture.rs"]
mod event_common;
use event_common::*;
use hf_storage::Error;

#[test]
fn unavailable_facts_and_unknown_or_pending_tasks_are_independent_of_stream_rows() {
    let (_fixture, store) = store();
    store
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: Vec::new(),
                partial_tail: 0,
                facts: facts(false),
            },
            deadline(),
        )
        .unwrap();
    let result = store
        .event_read(&query(10, false, EventFilter::Default, None), deadline())
        .unwrap();
    assert_eq!(result.lookup, Lookup::Found(id("event-task-1234")));
    let facts = result.facts.unwrap();
    assert_eq!(facts.state(), hf_domain::event_read::LogState::Unavailable);
    assert_eq!(facts.task_status.as_deref(), Some("running"));
    assert_eq!(facts.output_drained, None);
    let missing = query(10, false, EventFilter::Default, None);
    let mut missing = missing;
    missing.task = id("missing-task-1234");
    let result = store.event_read(&missing, deadline()).unwrap();
    assert_eq!(result.lookup, Lookup::Missing);
    assert!(result.facts.is_none());
}

#[test]
fn exact_logical_window_is_bounded_and_oversized_record_is_not_decoded() {
    let (_fixture, store) = store();
    let record = EventRecord {
        start: 0,
        end: 1_048_576,
        class: RecordClass::Oversized,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    store
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: vec![record],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let result = store
        .event_read(&query(20, false, EventFilter::All, None), deadline())
        .unwrap();
    assert_eq!(result.scanned_bytes, 1_048_576);
    assert_eq!(result.decoded, 0);
    assert_eq!(result.oversized, 1);
}

#[test]
fn fixture_refuses_inconsistent_record_spans_and_kind_metadata() {
    let (_fixture, store) = store();
    let bad = EventRecord {
        start: 0,
        end: 1,
        class: RecordClass::Malformed,
        seq: Some(1),
        kind: Some("assistant_text".into()),
        body: b"{}".to_vec(),
    };
    assert!(matches!(
        store.replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: vec![bad],
                partial_tail: 0,
                facts: facts(true)
            },
            deadline()
        ),
        Err(Error::InvalidEventQuery)
    ));
}

#[test]
fn empty_poll_cursor_survives_partial_growth_and_append_completion() {
    let (_fixture, store) = store();
    let task = id("event-task-1234");
    store
        .replace_event_fixture(
            &EventFixture {
                task: task.clone(),
                records: Vec::new(),
                partial_tail: 8,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let first = store
        .event_read(&query(10, false, EventFilter::Default, None), deadline())
        .unwrap();
    let cursor = first.newer_cursor.clone().unwrap();
    assert!(first.events.is_empty());
    assert!(!first.has_newer);
    store
        .append_event_fixture(
            &EventFixture {
                task: task.clone(),
                records: Vec::new(),
                partial_tail: 12,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let partial = store
        .event_read(
            &query(10, false, EventFilter::Default, Some(cursor.clone())),
            deadline(),
        )
        .unwrap();
    assert!(partial.events.is_empty());
    assert_eq!(partial.newer_cursor.unwrap().generation, cursor.generation);
    let completed = event_record(0, 1, "assistant_text");
    store
        .append_event_fixture(
            &EventFixture {
                task,
                records: vec![completed],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let poll = store
        .event_read(
            &query(10, false, EventFilter::Default, Some(cursor)),
            deadline(),
        )
        .unwrap();
    assert_eq!(
        poll.events
            .iter()
            .map(|r| r.seq.unwrap())
            .collect::<Vec<_>>(),
        [1]
    );
}

#[test]
fn replacement_expires_cursor_but_filter_mismatch_is_invalid_and_unknown_task_precedes_cursor() {
    let (_fixture, store) = store();
    let task = id("event-task-1234");
    let record = event_record(0, 1, "assistant_text");
    store
        .replace_event_fixture(
            &EventFixture {
                task: task.clone(),
                records: vec![record.clone()],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let page = store
        .event_read(&query(1, false, EventFilter::Default, None), deadline())
        .unwrap();
    let cursor = page.newer_cursor.unwrap();
    let changed = store.event_read(
        &query(1, false, EventFilter::All, Some(cursor.clone())),
        deadline(),
    );
    assert!(matches!(changed, Err(Error::InvalidEventQuery)));
    store
        .replace_event_fixture(
            &EventFixture {
                task: task.clone(),
                records: vec![record],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    assert!(matches!(
        store.event_read(
            &query(1, false, EventFilter::Default, Some(cursor.clone())),
            deadline()
        ),
        Err(Error::ExpiredEventCursor)
    ));
    let mut missing = query(1, false, EventFilter::Default, Some(cursor));
    missing.task = id("absent-task-1234");
    let missing = store.event_read(&missing, deadline()).unwrap();
    assert_eq!(missing.lookup, Lookup::Missing);
    assert!(missing.facts.is_none());
}

#[test]
fn pending_exact_task_wins_before_cursor_validation_and_has_no_stream_facts() {
    let fixture = Fixture::new();
    let store =
        Store::initialize_fixture(fixture.path().join("store"), default_limits(), deadline())
            .unwrap();
    let pending = id("event-pending-1234");
    store
        .replace_task_fixture(
            &TaskFixture {
                rows: Vec::new(),
                pending: vec![pending.clone()],
                coverage: Coverage {
                    discovery_complete: true,
                    ownership_complete: true,
                    source_complete: true,
                },
            },
            deadline(),
        )
        .unwrap();
    let cursor = hf_domain::event_read::EventCursor {
        store: "bad".into(),
        generation: "bad".into(),
        task: pending.clone(),
        kinds: vec!["future".into()],
        direction: hf_domain::event_read::Direction::Older,
        boundary: u64::MAX,
        resync: false,
        neighbour_seq: None,
        neighbour_skip: false,
    };
    let mut pending_query = query(10, false, EventFilter::Default, Some(cursor));
    pending_query.task = pending.clone();
    let result = store.event_read(&pending_query, deadline()).unwrap();
    assert_eq!(result.lookup, Lookup::Pending(pending));
    assert!(result.facts.is_none());
    assert!(result.events.is_empty());
}
