#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common/event_fixture.rs"]
mod event_common;
use event_common::*;

#[test]
fn pages_account_filtered_records_in_prefix_and_reverse_cursor_is_reversible() {
    let (_fixture, store) = store();
    let mut records = Vec::new();
    let mut start = 0;
    for (seq, kind) in [
        (0, "assistant_text"),
        (1, "assistant_delta"),
        (2, "tool_call"),
        (3, "assistant_text"),
    ] {
        let record = event_record(start, seq, kind);
        start = record.end;
        records.push(record);
    }
    store
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records,
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let first = store
        .event_read(&query(3, true, EventFilter::Default, None), deadline())
        .unwrap();
    assert_eq!(first.lookup, Lookup::Found(id("event-task-1234")));
    assert_eq!(first.span, Some((0, 2)));
    assert_eq!(first.decoded, 3);
    assert_eq!(
        first
            .events
            .iter()
            .map(|e| e.seq.unwrap())
            .collect::<Vec<_>>(),
        [0, 2]
    );
    let next = store
        .event_read(
            &query(2, false, EventFilter::Default, first.newer_cursor),
            deadline(),
        )
        .unwrap();
    assert_eq!(
        next.events
            .iter()
            .map(|e| e.seq.unwrap())
            .collect::<Vec<_>>(),
        [3]
    );
    let back = store
        .event_read(
            &query(2, false, EventFilter::Default, next.older_cursor),
            deadline(),
        )
        .unwrap();
    assert!(
        back.events
            .iter()
            .map(|e| e.seq.unwrap())
            .collect::<Vec<_>>()
            .contains(&2)
    );
    assert!(first.events.last().unwrap().end <= next.events.first().unwrap().start);
}

#[test]
fn unknown_kind_and_skipped_lines_consume_decode_budget_without_being_returned() {
    let (_fixture, store) = store();
    let unknown = event_record(0, 1, "future_kind");
    let blank_start = unknown.end;
    let blank = EventRecord {
        start: blank_start,
        end: blank_start + 1,
        class: RecordClass::Blank,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    let malformed_start = blank.end;
    let malformed = EventRecord {
        start: malformed_start,
        end: malformed_start + 5,
        class: RecordClass::Malformed,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    store
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: vec![unknown, blank, malformed],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let result = store
        .event_read(&query(3, true, EventFilter::All, None), deadline())
        .unwrap();
    assert!(result.events.is_empty());
    assert_eq!(result.decoded, 3);
    assert_eq!(result.span, Some((1, 1)));
    assert_eq!(result.malformed, 1);
}

#[test]
fn malformed_line_gap_reason_is_carried_across_page_boundary() {
    let (_fixture, store) = store();
    let first = event_record(0, 1, "assistant_text");
    let malformed_start = first.end;
    let malformed = EventRecord {
        start: malformed_start,
        end: malformed_start + 8,
        class: RecordClass::Malformed,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    let last = event_record(malformed.end, 3, "tool_call");
    store
        .replace_event_fixture(
            &EventFixture {
                task: id("event-task-1234"),
                records: vec![first, malformed, last],
                partial_tail: 0,
                facts: facts(true),
            },
            deadline(),
        )
        .unwrap();
    let before = store
        .event_read(&query(2, true, EventFilter::All, None), deadline())
        .unwrap();
    assert_eq!(before.malformed, 1);
    let after = store
        .event_read(
            &query(2, false, EventFilter::All, before.newer_cursor),
            deadline(),
        )
        .unwrap();
    assert_eq!(after.events[0].seq, Some(3));
    assert_eq!(
        after.gaps,
        [hf_domain::event_read::EventGap {
            after_seq: 1,
            before_seq: 3,
            reason: hf_domain::event_read::GapReason::UnreadableRecords
        }]
    );
}
