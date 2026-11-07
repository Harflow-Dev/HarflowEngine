#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The event-log fixture writers: truncation by physical line, removal, and the stream
//! facts copied from the task.
mod common;
mod fixture_support;
use fixture_support::{
    A, B, C, World, done, event, event_len, later, one, read, records, stream, stream_generation,
    task, tid, world,
};
use hf_storage::{Error, FixtureLine};

#[test]
fn truncation_counts_an_unterminated_final_line_as_a_physical_line() {
    let seed = |w: &World| {
        w.add(&done(A));
        w.append(A, &event(0, "task_started"));
        w.append(A, &event(1, "assistant_text"));
        w.append(A, &FixtureLine::Malformed { len: 9 });
        w.append(A, &FixtureLine::Partial { len: 11 });
    };
    let e0 = event_len(0, "task_started");
    let e1 = e0 + event_len(1, "assistant_text");
    let e2 = e1 + 9;
    let cases: [(u64, i64, usize, i64); 7] = [
        (0, 0, 0, 0),
        (1, e0, 1, 0),
        (2, e1, 2, 0),
        (3, e2, 3, 0),
        (4, e2, 3, 11),
        (5, e2, 3, 11),
        (500, e2, 3, 11),
    ];
    for (keep, end, count, tail) in cases {
        let w = world();
        seed(&w);
        w.store
            .truncate_event_log_fixture(&tid(A), keep, later(5))
            .unwrap();
        assert_eq!(records(&w, A).len(), count, "keep {keep}");
        assert_eq!(
            stream(&w, A),
            format!("{end}|{tail}|1|2|1|NULL|0|'completed'"),
            "keep {keep}"
        );
    }
}

#[test]
fn truncating_a_log_without_a_partial_keeps_everything_when_keep_exceeds_the_lines() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let end = event_len(0, "task_started");
    w.store
        .truncate_event_log_fixture(&tid(A), 2, later(5))
        .unwrap();
    assert_eq!(stream(&w, A), format!("{end}|0|1|2|1|NULL|0|'completed'"));
    assert_eq!(records(&w, A).len(), 1);
}

#[test]
fn an_append_after_a_truncate_starts_at_the_new_end() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.append(A, &event(1, "assistant_text"));
    w.store
        .truncate_event_log_fixture(&tid(A), 1, later(5))
        .unwrap();
    w.append(A, &event(7, "usage"));
    let e0 = event_len(0, "task_started");
    assert_eq!(
        records(&w, A),
        [
            format!("0|{e0}|0|0|'task_started'|{}", e0 - 1),
            format!(
                "{e0}|{}|0|7|'usage'|{}",
                e0 + event_len(7, "usage"),
                event_len(7, "usage") - 1
            ),
        ]
    );
}

#[test]
fn truncating_a_missing_log_or_task_is_refused() {
    let w = world();
    w.add(&done(A));
    for id in [A, C] {
        let error = w
            .store
            .truncate_event_log_fixture(&tid(id), 1, later(5))
            .unwrap_err();
        assert!(matches!(error, Error::FixtureMissing), "{id}: {error:?}");
    }
}

#[test]
fn removal_deletes_one_stream_and_its_records_and_a_second_removal_is_refused() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    for id in [A, B] {
        w.append(id, &event(0, "task_started"));
        w.append(id, &FixtureLine::Partial { len: 11 });
    }
    w.store.remove_event_log_fixture(&tid(A), later(5)).unwrap();
    assert_eq!(
        fixture_support::count(&w.db(), "event_streams", "task", A),
        0
    );
    assert!(records(&w, A).is_empty());
    assert_eq!(records(&w, B).len(), 1);
    assert_eq!(
        stream(&w, B),
        format!(
            "{}|11|1|2|1|NULL|0|'completed'",
            event_len(0, "task_started")
        )
    );
    let error = w
        .store
        .remove_event_log_fixture(&tid(A), later(5))
        .unwrap_err();
    assert!(matches!(error, Error::FixtureMissing), "{error:?}");
    let result = read(&w, A, true, None).unwrap();
    assert_eq!(
        (result.facts.unwrap().available, result.events.len()),
        (false, 0)
    );
}

#[test]
fn a_log_recreated_after_removal_starts_empty_with_a_new_generation() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let old = stream_generation(&w, A);
    w.store.remove_event_log_fixture(&tid(A), later(5)).unwrap();
    w.append(A, &event(0, "task_started"));
    assert_ne!(stream_generation(&w, A), old);
    assert_eq!(records(&w, A).len(), 1);
    assert!(records(&w, A)[0].starts_with("0|"));
}

#[test]
fn stream_facts_are_copied_from_the_task_with_the_documented_capture_mapping() {
    // (status, exit code, drained) -> capture 2 = observed_terminal, 3 = unknown.
    let table: [(&str, Option<i64>, bool, i64); 12] = [
        ("completed", Some(0), true, 2),
        ("failed", Some(1), false, 2),
        ("timed_out", Some(124), true, 2),
        ("cancelled", Some(130), false, 2),
        ("completed", None, true, 3),
        ("failed", None, false, 3),
        ("timed_out", None, true, 3),
        ("cancelled", None, false, 3),
        ("running", None, false, 3),
        ("running", Some(0), false, 3),
        ("queued", Some(0), true, 3),
        ("mystery", Some(0), true, 3),
    ];
    for (status, exit_code, drained, capture) in table {
        let w = world();
        w.add(&task(A, status, exit_code, drained));
        w.append(A, &event(0, "task_started"));
        let expected = format!(
            "{}|0|1|{capture}|{}|NULL|0|'{status}'",
            event_len(0, "task_started"),
            i32::from(drained)
        );
        assert_eq!(stream(&w, A), expected, "{status} {exit_code:?}");
    }
}

#[test]
fn stream_facts_never_claim_a_live_capture_or_a_loss_and_never_a_grace() {
    let w = world();
    for (i, status) in ["running", "completed", "failed"].into_iter().enumerate() {
        let id = format!("{i}{}", &A[1..]);
        w.add(&task(&id, status, (status != "running").then_some(0), true));
        w.append(&id, &event(0, "task_started"));
        let (capture, grace, eof): (i64, i64, Option<i64>) = w
            .db()
            .query_row(
                "SELECT capture,grace,eof_seq FROM event_streams WHERE task=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(
            capture == 2 || capture == 3,
            "{status}: capture {capture} is never live (0) or lost (1)"
        );
        assert_eq!((grace, eof), (0, None));
    }
}

#[test]
fn eof_seq_stays_null_after_a_task_finished_line_and_appending_still_works() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    w.append(A, &event(1, "task_finished"));
    let eof: Option<i64> = one(
        &w.db(),
        "SELECT eof_seq FROM event_streams WHERE task=?1",
        [A],
    );
    assert_eq!(eof, None);
    let facts = read(&w, A, true, None).unwrap().facts.unwrap();
    assert_eq!(
        facts.eof_finished_seq,
        Some(1),
        "the reader derives it from the last record"
    );
    w.append(A, &event(2, "assistant_text"));
    let facts = read(&w, A, true, None).unwrap().facts.unwrap();
    assert_eq!(facts.eof_finished_seq, None);
}

#[test]
fn appending_to_a_damaged_task_is_refused_and_commits_nothing() {
    let w = world();
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    for line in [
        event(0, "task_started"),
        FixtureLine::Partial { len: 11 },
        FixtureLine::Malformed { len: 9 },
    ] {
        let error = w
            .store
            .append_event_record_fixture(&tid(A), &line, later(5))
            .unwrap_err();
        assert!(matches!(error, Error::FixtureMissing), "{error:?}");
    }
    assert_eq!(
        fixture_support::count(&w.db(), "event_streams", "task", A),
        0
    );
    assert!(records(&w, A).is_empty());
}

#[test]
fn a_log_written_before_damage_stays_readable_and_removable() {
    let w = world();
    w.add(&done(A));
    w.append(A, &event(0, "task_started"));
    let before = stream(&w, A);
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    assert_eq!(stream(&w, A), before);
    let error = w
        .store
        .append_event_record_fixture(&tid(A), &event(1, "usage"), later(5))
        .unwrap_err();
    assert!(matches!(error, Error::FixtureMissing), "{error:?}");
    assert_eq!(stream(&w, A), before);
    w.store
        .truncate_event_log_fixture(&tid(A), 0, later(5))
        .unwrap();
    assert!(records(&w, A).is_empty());
    w.store.remove_event_log_fixture(&tid(A), later(5)).unwrap();
    assert_eq!(
        fixture_support::count(&w.db(), "event_streams", "task", A),
        0
    );
}

#[test]
fn facts_are_taken_from_the_task_at_each_write() {
    let w = world();
    w.add(&task(A, "completed", Some(0), false));
    w.append(A, &event(0, "task_started"));
    assert!(stream(&w, A).ends_with("|1|2|0|NULL|0|'completed'"));
}

#[test]
fn a_truncate_that_removes_nothing_still_expires_a_cursor_inside_the_log() {
    let w = world();
    w.add(&done(A));
    for (seq, kind) in [(0, "task_started"), (1, "assistant_text"), (2, "tool_call")] {
        w.append(A, &event(seq, kind));
    }
    let page = read(&w, A, true, None).unwrap();
    let cursor = page.newer_cursor.unwrap();
    w.store
        .truncate_event_log_fixture(&tid(A), 3, later(5))
        .unwrap();
    let error = read(&w, A, false, Some(cursor)).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
}
