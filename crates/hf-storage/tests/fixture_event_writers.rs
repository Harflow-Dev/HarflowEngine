#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The event-log fixture writers: appended lines, partial-tail joins, line classes by length
//! and stream generations across appends.
mod common;
mod fixture_support;
use fixture_support::{
    A, done, event, event_body, event_len, is_hex32, later, one, read, records, stream,
    stream_generation, tid, world,
};
use hf_storage::{Error, FixtureLine};

#[test]
fn consecutive_events_are_contiguous_valid_records_with_their_body_bytes() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.append(A, &event(1, "assistant_text"));
    let first = event_len(0, "task_started");
    let second = event_len(1, "assistant_text");
    assert_eq!(
        records(&w, A),
        [
            format!("0|{first}|0|0|'task_started'|{}", first - 1),
            format!(
                "{first}|{}|0|1|'assistant_text'|{}",
                first + second,
                second - 1
            ),
        ]
    );
    assert_eq!(
        stream(&w, A),
        format!("{}|0|1|2|1|NULL|0|'completed'", first + second)
    );
    let body: Vec<u8> = one(
        &w.db(),
        "SELECT body FROM event_records WHERE task=?1 AND seq=1",
        [A],
    );
    assert_eq!(body, event_body(1, "assistant_text"));
}

#[test]
fn the_first_append_creates_the_stream_and_a_missing_task_is_refused() {
    let w = world();
    let error = w
        .store
        .append_event_record_fixture(&tid(A), &event(0, "task_started"), later(5))
        .unwrap_err();
    assert!(matches!(error, Error::FixtureMissing), "{error:?}");
    w.add(&done(A));
    assert_eq!(
        fixture_support::count(&w.db(), "event_streams", "task", A),
        0
    );
    w.append(A, &event(0, "task_started"));
    assert_eq!(
        fixture_support::count(&w.db(), "event_streams", "task", A),
        1
    );
}

#[test]
fn an_append_keeps_the_stream_generation_and_a_truncate_rotates_it() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let created = stream_generation(&w, A);
    assert!(is_hex32(&created), "{created}");
    w.append(A, &event(1, "assistant_text"));
    assert_eq!(stream_generation(&w, A), created);
    w.append(A, &FixtureLine::Partial { len: 11 });
    assert_eq!(stream_generation(&w, A), created);
    w.append(A, &FixtureLine::Malformed { len: 9 });
    assert_eq!(stream_generation(&w, A), created);
    w.store
        .truncate_event_log_fixture(&tid(A), 2, later(5))
        .unwrap();
    let rotated = stream_generation(&w, A);
    assert!(is_hex32(&rotated), "{rotated}");
    assert_ne!(rotated, created);
    w.append(A, &event(2, "assistant_text"));
    assert_eq!(
        stream_generation(&w, A),
        rotated,
        "an append after a truncate preserves the new one"
    );
    w.store
        .truncate_event_log_fixture(&tid(A), 100, later(5))
        .unwrap();
    assert_ne!(
        stream_generation(&w, A),
        rotated,
        "even a truncate that removes nothing rotates"
    );
}

#[test]
fn an_append_does_not_expire_a_cursor_and_a_truncate_or_removal_does() {
    let w = world();
    w.add(&done(A));
    for (seq, kind) in [(0, "task_started"), (1, "assistant_text"), (2, "tool_call")] {
        w.append(A, &event(seq, kind));
    }
    let cursor = read(&w, A, true, None).unwrap().newer_cursor.unwrap();
    w.append(A, &event(3, "assistant_text"));
    let after = read(&w, A, false, Some(cursor.clone())).unwrap();
    let seqs: Vec<u64> = after.events.iter().filter_map(|e| e.seq).collect();
    assert_eq!(seqs, [3]);
    w.store
        .truncate_event_log_fixture(&tid(A), 1, later(5))
        .unwrap();
    let error = read(&w, A, false, Some(cursor.clone())).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
    w.store.remove_event_log_fixture(&tid(A), later(5)).unwrap();
    let error = read(&w, A, false, Some(cursor)).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
}

#[test]
fn a_partial_line_adds_to_the_tail_and_never_to_the_complete_end() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let first = event_len(0, "task_started");
    w.append(A, &FixtureLine::Partial { len: 11 });
    assert_eq!(records(&w, A).len(), 1);
    assert_eq!(
        stream(&w, A),
        format!("{first}|11|1|2|1|NULL|0|'completed'")
    );
    w.append(A, &FixtureLine::Partial { len: 11 });
    assert_eq!(
        stream(&w, A),
        format!("{first}|22|1|2|1|NULL|0|'completed'")
    );
}

#[test]
fn a_partial_joins_the_next_terminated_event_and_the_joined_line_is_malformed() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let first = event_len(0, "task_started");
    w.append(A, &FixtureLine::Partial { len: 11 });
    w.append(A, &event(1, "assistant_text"));
    let joined = 11 + event_len(1, "assistant_text");
    assert_eq!(
        records(&w, A),
        [
            format!("0|{first}|0|0|'task_started'|{}", first - 1),
            format!("{first}|{}|2|NULL|NULL|0", first + joined),
        ]
    );
    assert_eq!(
        stream(&w, A),
        format!("{}|0|1|2|1|NULL|0|'completed'", first + joined)
    );
    w.append(A, &event(2, "assistant_text"));
    let last = records(&w, A).pop().unwrap();
    assert!(
        last.starts_with(&format!("{}|", first + joined))
            && last.contains("|0|2|'assistant_text'|"),
        "{last}"
    );
}

#[test]
fn a_partial_joins_a_malformed_line_into_one_malformed_record() {
    let w = world();
    w.add(&done(A));
    w.append(A, &FixtureLine::Partial { len: 11 });
    w.append(A, &FixtureLine::Malformed { len: 9 });
    assert_eq!(records(&w, A), ["0|20|2|NULL|NULL|0"]);
    assert_eq!(stream(&w, A), "20|0|1|2|1|NULL|0|'completed'");
}

#[test]
fn a_malformed_line_is_a_hidden_record_that_the_reader_counts() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.append(A, &FixtureLine::Malformed { len: 9 });
    w.append(A, &event(5, "assistant_text"));
    let result = read(&w, A, true, None).unwrap();
    let seqs: Vec<u64> = result.events.iter().filter_map(|e| e.seq).collect();
    assert_eq!(
        (seqs, result.malformed, result.oversized),
        (vec![0, 5], 1, 0)
    );
}

#[test]
fn line_classes_follow_the_length_boundaries() {
    let w = world();
    w.add(&done(A));
    w.append(A, &FixtureLine::Malformed { len: 1 });
    w.append(A, &FixtureLine::Malformed { len: 1_000_001 });
    w.append(A, &FixtureLine::Malformed { len: 1_000_002 });
    w.append(A, &FixtureLine::Malformed { len: 2 });
    assert_eq!(
        records(&w, A),
        [
            "0|1|1|NULL|NULL|0".to_owned(),
            "1|1000002|2|NULL|NULL|0".to_owned(),
            "1000002|2000004|3|NULL|NULL|0".to_owned(),
            "2000004|2000006|2|NULL|NULL|0".to_owned(),
        ]
    );
}

#[test]
fn a_partial_tail_that_pushes_a_line_over_the_limit_makes_it_oversized() {
    let w = world();
    w.add(&done(A));
    w.append(A, &FixtureLine::Partial { len: 1_000_000 });
    w.append(A, &FixtureLine::Malformed { len: 9 });
    assert_eq!(records(&w, A), ["0|1000009|3|NULL|NULL|0"]);
}

fn invalid_events() -> Vec<(&'static str, FixtureLine)> {
    let line = |seq: u64, kind: String, body: Vec<u8>| FixtureLine::Event { seq, kind, body };
    vec![
        (
            "seq beyond i64",
            line(i64::MAX as u64 + 1, "usage".into(), b"{}".to_vec()),
        ),
        (
            "seq u64::MAX",
            line(u64::MAX, "usage".into(), b"{}".to_vec()),
        ),
        (
            "body over 16384 bytes",
            line(1, "usage".into(), vec![b'x'; 16385]),
        ),
        (
            "kind over 128 bytes",
            line(1, "k".repeat(129), b"{}".to_vec()),
        ),
        (
            "body that is not UTF-8",
            line(1, "usage".into(), vec![0xff]),
        ),
    ]
}

#[test]
fn an_invalid_event_is_refused_with_nothing_committed_and_the_next_append_works() {
    let w = world();
    w.add(&done(A));
    for (name, line) in invalid_events() {
        let error = w
            .store
            .append_event_record_fixture(&tid(A), &line, later(5))
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidEventQuery),
            "{name}: {error:?}"
        );
        assert_eq!(
            fixture_support::count(&w.db(), "event_streams", "task", A),
            0,
            "{name}: no stream is created"
        );
    }
    w.append(A, &event(0, "task_started"));
    let (before_stream, before_records) = (stream(&w, A), records(&w, A));
    let generation = stream_generation(&w, A);
    for (name, line) in invalid_events() {
        let error = w
            .store
            .append_event_record_fixture(&tid(A), &line, later(5))
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidEventQuery),
            "{name}: {error:?}"
        );
        assert_eq!(stream(&w, A), before_stream, "{name}");
        assert_eq!(records(&w, A), before_records, "{name}");
        assert_eq!(stream_generation(&w, A), generation, "{name}");
    }
    w.append(A, &event(1, "assistant_text"));
    assert_eq!(records(&w, A).len(), 2);
    assert_eq!(stream_generation(&w, A), generation);
}

#[test]
fn events_at_the_validation_limits_are_accepted() {
    let w = world();
    w.add(&done(A));
    w.append(
        A,
        &FixtureLine::Event {
            seq: i64::MAX as u64,
            kind: "k".repeat(128),
            body: vec![b'x'; 16384],
        },
    );
    let rows = records(&w, A);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0],
        format!("0|16385|0|{}|'{}'|16384", i64::MAX, "k".repeat(128))
    );
}
