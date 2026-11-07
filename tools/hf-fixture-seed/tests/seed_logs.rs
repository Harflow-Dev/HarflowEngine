#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The log states of the conformance world's tasks, read back through `ReadSession`.
mod support;
use hf_domain::{
    event_read::{CaptureState, EventGap, GapReason, LogState},
    task_read::Lookup,
};
use support::{A, B, C, event_for, events, events_with, ok, seeded, seqs, serial, task, tid};

#[test]
fn log_state_of_a_final_log_is_final_with_the_default_kinds_only() {
    let _serial = serial();
    let scratch = seeded();
    let result = events(&scratch.root, A);
    assert_eq!(
        seqs(&result),
        [0, 3, 4, 5, 6, 7],
        "assistant_delta is hidden by default"
    );
    let kinds: Vec<_> = result
        .events
        .iter()
        .map(|e| e.kind.clone().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "task_started",
            "assistant_text",
            "tool_call",
            "tool_result",
            "usage",
            "task_finished"
        ]
    );
    let facts = result.facts.unwrap();
    assert_eq!(facts.task_status.as_deref(), Some("completed"));
    assert_eq!(
        (
            facts.available,
            facts.capture,
            facts.output_drained,
            facts.eof_finished_seq,
            facts.within_grace
        ),
        (
            true,
            CaptureState::ObservedTerminal,
            Some(true),
            Some(7),
            false
        )
    );
    assert_eq!(facts.state(), LogState::Final);
    assert_eq!(
        (
            result.span,
            result.malformed,
            result.oversized,
            result.gaps.len()
        ),
        (Some((0, 7)), 0, 0, 0)
    );
    assert!(!result.has_older && !result.has_newer && result.older_cursor.is_none());
}

#[test]
fn the_newest_page_returns_the_last_events_in_ascending_order() {
    let _serial = serial();
    let scratch = seeded();
    let result = events_with(&scratch.root, A, 3, false, None).unwrap();
    assert_eq!(seqs(&result), [5, 6, 7]);
    assert_eq!(
        (result.span, result.has_older, result.has_newer),
        (Some((5, 7)), true, false)
    );
    assert!(result.older_cursor.is_some() && result.newer_cursor.is_some());
}

#[test]
fn forward_paging_covers_the_log_once_and_polling_returns_the_same_cursor() {
    let _serial = serial();
    let scratch = seeded();
    let mut page = events_with(&scratch.root, A, 2, true, None).unwrap();
    assert_eq!(
        (seqs(&page), page.span, page.has_older, page.has_newer),
        (vec![0], Some((0, 1)), false, true)
    );
    assert!(page.older_cursor.is_none());
    let mut collected = seqs(&page);
    while page.has_newer {
        page = events_with(&scratch.root, A, 2, false, page.newer_cursor.clone()).unwrap();
        collected.extend(seqs(&page));
    }
    assert_eq!(collected, [0, 3, 4, 5, 6, 7]);
    let poll = events_with(&scratch.root, A, 2, false, page.newer_cursor.clone()).unwrap();
    assert_eq!((poll.events.len(), poll.has_newer), (0, false));
    assert_eq!(poll.newer_cursor, page.newer_cursor);
}

#[test]
fn log_state_of_a_failed_task_without_a_finish_event_is_incomplete() {
    let _serial = serial();
    let scratch = seeded();
    let result = events(&scratch.root, B);
    assert_eq!(seqs(&result), [0, 1, 2]);
    let facts = result.facts.unwrap();
    assert_eq!(facts.task_status.as_deref(), Some("failed"));
    assert_eq!(
        (
            facts.capture,
            facts.output_drained,
            facts.eof_finished_seq,
            facts.within_grace
        ),
        (CaptureState::ObservedTerminal, Some(true), None, false)
    );
    assert_eq!(facts.state(), LogState::Incomplete);
}

#[test]
fn a_task_without_a_log_is_unavailable_with_no_cursors() {
    let _serial = serial();
    let scratch = seeded();
    let result = events(&scratch.root, C);
    assert_eq!(result.lookup, Lookup::Found(tid(C)));
    assert!(result.events.is_empty() && result.span.is_none());
    assert!(result.older_cursor.is_none() && result.newer_cursor.is_none());
    assert!((!result.has_older) && (!result.has_newer));
    assert_eq!(result.facts.unwrap().state(), LogState::Unavailable);
}

#[test]
fn a_gappy_log_skips_the_malformed_line_and_reports_the_gap() {
    let _serial = serial();
    let scratch = seeded();
    let gappy = "12345678-0000-4000-8000-000000000001";
    ok(
        &scratch.root,
        "add_task",
        &format!("{{\"spec\":{}}}", task(gappy, &[("prompt", "\"p\"")])),
    );
    let append = |seq: u64, kind: &str, fields: &str| {
        ok(
            &scratch.root,
            "append_event",
            &event_for(gappy, seq, kind, fields),
        );
    };
    append(0, "task_started", "{}");
    append(1, "assistant_text", r#"{"text":"a"}"#);
    ok(
        &scratch.root,
        "append_malformed_line",
        &format!("{{\"task_id\":\"{gappy}\"}}"),
    );
    append(5, "assistant_text", r#"{"text":"b"}"#);
    append(
        6,
        "task_finished",
        r#"{"status":"completed","exit_code":0}"#,
    );
    let result = events(&scratch.root, gappy);
    assert_eq!(seqs(&result), [0, 1, 5, 6]);
    assert_eq!((result.malformed, result.oversized), (1, 0));
    assert_eq!(
        result.gaps,
        [EventGap {
            after_seq: 1,
            before_seq: 5,
            reason: GapReason::UnreadableRecords
        }]
    );
    assert_eq!(result.facts.unwrap().state(), LogState::Final);
}

#[test]
fn a_damaged_task_leaves_its_log_unreachable_through_the_read_path() {
    let _serial = serial();
    let scratch = seeded();
    ok(
        &scratch.root,
        "damage_task",
        &format!("{{\"task_id\":\"{A}\"}}"),
    );
    let result = events(&scratch.root, A);
    assert_eq!(result.lookup, Lookup::Missing);
    assert!(result.events.is_empty() && result.facts.is_none());
    let out = support::run(
        &scratch.root,
        "append_event",
        &event_for(A, 8, "usage", "{}"),
    );
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
    assert!(out.stderr.contains("FixtureMissing"), "{}", out.stderr);
    assert_eq!(
        ok(
            &scratch.root,
            "remove_log",
            &format!("{{\"task_id\":\"{A}\"}}")
        ),
        "null\n"
    );
}
