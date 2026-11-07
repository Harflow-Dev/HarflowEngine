#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Mutating sequences through the real process, observed through `ReadSession`: cursor
//! expiry across append, partial, truncate and remove; and the digest's independence from
//! reads and from edits made outside the root.
mod support;
use hf_domain::event_read::{EventGap, GapReason, LogState};
use hf_storage::{Error, ReadRefusal};
use support::{
    A, B, Scratch, digest, event_for, events, events_with, ok, run, seeded, seqs, serial, task,
    try_open,
};

fn id(task: &str) -> String {
    format!("{{\"task_id\":\"{task}\"}}")
}

#[test]
fn append_partial_truncate_and_remove_expire_the_cursor_exactly_when_the_stream_is_replaced() {
    let _serial = serial();
    let scratch = seeded();
    let page = events_with(&scratch.root, A, 4, true, None).unwrap();
    assert_eq!(
        seqs(&page),
        [0, 3],
        "lines 0 to 3 decode; the deltas are hidden"
    );
    let cursor = page.newer_cursor.clone().unwrap();

    ok(
        &scratch.root,
        "append_event",
        &event_for(A, 8, "assistant_text", r#"{"text":"late"}"#),
    );
    ok(&scratch.root, "append_partial_line", &id(A));
    let after = events_with(&scratch.root, A, 50, false, Some(cursor.clone())).unwrap();
    assert_eq!(
        seqs(&after),
        [4, 5, 6, 7, 8],
        "an append keeps the cursor and the partial tail is held back"
    );
    assert!(!after.has_newer);
    assert_eq!(after.facts.unwrap().state(), LogState::Incomplete);

    truncate_task(&scratch, A, 1);
    let error = events_with(&scratch.root, A, 50, false, Some(cursor.clone())).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
    let fresh = events(&scratch.root, A);
    assert_eq!(seqs(&fresh), [0]);
    assert_eq!(fresh.facts.unwrap().state(), LogState::Incomplete);

    assert_eq!(ok(&scratch.root, "remove_log", &id(A)), "null\n");
    let error = events_with(&scratch.root, A, 50, false, Some(cursor)).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
    let gone = events(&scratch.root, A);
    assert!(gone.events.is_empty());
    assert_eq!(gone.facts.unwrap().state(), LogState::Unavailable);
}

#[test]
fn a_cursor_survives_an_append_and_a_second_append_after_a_finished_log() {
    let _serial = serial();
    let scratch = seeded();
    let cursor = events_with(&scratch.root, A, 200, true, None)
        .unwrap()
        .newer_cursor
        .unwrap();
    ok(
        &scratch.root,
        "append_event",
        &event_for(A, 8, "usage", r#"{"input_tokens":1}"#),
    );
    ok(
        &scratch.root,
        "append_event",
        &event_for(A, 9, "usage", r#"{"input_tokens":2}"#),
    );
    let after = events_with(&scratch.root, A, 50, false, Some(cursor)).unwrap();
    assert_eq!(seqs(&after), [8, 9]);
    assert_eq!(
        after.facts.unwrap().eof_finished_seq,
        None,
        "the last line is no longer task_finished"
    );
}

#[test]
fn a_partial_tail_joins_the_next_event_into_one_malformed_line() {
    let _serial = serial();
    let scratch = seeded();
    ok(&scratch.root, "append_partial_line", &id(A));
    let held = events(&scratch.root, A);
    assert_eq!(
        (seqs(&held), held.malformed),
        (vec![0, 3, 4, 5, 6, 7], 0),
        "a partial is not yet a line"
    );
    ok(
        &scratch.root,
        "append_event",
        &event_for(A, 8, "usage", r#"{"input_tokens":1}"#),
    );
    let joined = events(&scratch.root, A);
    assert_eq!(
        (seqs(&joined), joined.malformed),
        (vec![0, 3, 4, 5, 6, 7], 1),
        "the event is swallowed"
    );
    ok(
        &scratch.root,
        "append_event",
        &event_for(A, 9, "usage", r#"{"input_tokens":2}"#),
    );
    let later = events(&scratch.root, A);
    assert_eq!(seqs(&later), [0, 3, 4, 5, 6, 7, 9]);
    assert_eq!(
        later.gaps,
        [EventGap {
            after_seq: 7,
            before_seq: 9,
            reason: GapReason::UnreadableRecords
        }]
    );
}

#[test]
fn truncating_to_n_keeps_n_physical_lines_including_an_unterminated_one() {
    let _serial = serial();
    let scratch = seeded();
    ok(&scratch.root, "append_partial_line", &id(B));
    truncate_task(&scratch, B, 3);
    let kept = events(&scratch.root, B);
    assert_eq!(seqs(&kept), [0, 1, 2]);
    truncate_task(&scratch, B, 4);
    assert_eq!(
        seqs(&events(&scratch.root, B)),
        [0, 1, 2],
        "keep 4 would also keep the partial; it is already gone"
    );
    ok(&scratch.root, "append_partial_line", &id(B));
    truncate_task(&scratch, B, 4);
    ok(
        &scratch.root,
        "append_event",
        &event_for(B, 3, "notice", r#"{"text":"x"}"#),
    );
    let joined = events(&scratch.root, B);
    assert_eq!(
        (seqs(&joined), joined.malformed),
        (vec![0, 1, 2], 1),
        "the surviving partial still joins"
    );
}

fn truncate_task(scratch: &Scratch, task: &str, keep: u64) {
    let body = format!("{{\"task_id\":\"{task}\",\"keep_events\":{keep}}}");
    assert_eq!(ok(&scratch.root, "truncate_log", &body), "null\n");
}

#[test]
fn removing_a_log_leaves_the_other_logs_and_a_second_removal_fails() {
    let _serial = serial();
    let scratch = seeded();
    ok(&scratch.root, "remove_log", &id(A));
    assert_eq!(seqs(&events(&scratch.root, B)), [0, 1, 2]);
    let out = run(&scratch.root, "remove_log", &id(A));
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
    assert!(out.stderr.contains("FixtureMissing"), "{}", out.stderr);
}

#[test]
fn the_digest_is_unchanged_by_a_real_read_only_open_and_read() {
    let _serial = serial();
    let scratch = seeded();
    let before = digest(&scratch.root);
    for _ in 0..2 {
        let session = try_open(&scratch.root).unwrap();
        drop(session);
        support::page(&scratch.root, 100, false, None, None);
        events(&scratch.root, A);
        events_with(&scratch.root, B, 1, false, None).unwrap();
        support::lookup(&scratch.root, &["aaaaaaaa", "zz"]);
    }
    assert_eq!(digest(&scratch.root), before);
    assert_eq!(ok(&scratch.root, "root_state", "{}"), "\"initialized\"\n");
    assert_eq!(digest(&scratch.root), before);
}

#[test]
fn the_digest_changes_on_append_partial_line_and_the_seed_reads_back_afterwards() {
    let _serial = serial();
    let scratch = seeded();
    let before = digest(&scratch.root);
    ok(&scratch.root, "append_partial_line", &id(A));
    let after = digest(&scratch.root);
    assert_ne!(after, before);
    assert_eq!(after.len(), 64);
    assert!(
        after
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(seqs(&events(&scratch.root, A)), [0, 3, 4, 5, 6, 7]);
}

#[test]
fn changing_an_artifact_target_edits_only_the_file_outside_the_root() {
    let _serial = serial();
    let scratch = seeded();
    let spec = format!(
        "{{\"spec\":{}}}",
        task(
            "13131313-1313-4313-8313-131313131313",
            &[("real_workspace", "true")]
        )
    );
    let repo = ok(&scratch.root, "add_task", &spec);
    let repo = repo
        .trim()
        .strip_prefix("{\"repo_path\":\"")
        .unwrap()
        .strip_suffix("\"}")
        .unwrap()
        .to_owned();
    assert_eq!(
        repo,
        format!(
            "{}/repos/13131313-1313-4313-8313-131313131313",
            scratch.base.display()
        )
    );
    let item = r#"{"artifact_id":"x1","name":"a.txt","target":"present","kind":"report","note":"n","registered_at":"2026-01-10T10:04:00+00:00","provenance_method":"verified_process_lineage"}"#;
    let body =
        format!("{{\"task_id\":\"13131313-1313-4313-8313-131313131313\",\"items\":[{item}]}}");
    assert_eq!(
        ok(&scratch.root, "write_artifacts", &body),
        format!("{{\"paths\":[\"{repo}/out/a.txt\"]}}\n")
    );
    let target = std::path::Path::new(&repo).join("out/a.txt");
    assert_eq!(std::fs::read(&target).unwrap(), b"artifact bytes");
    let before = digest(&scratch.root);
    let change = "{\"task_id\":\"13131313-1313-4313-8313-131313131313\",\"name\":\"a.txt\"}";
    assert_eq!(
        ok(&scratch.root, "change_artifact_target", change),
        "null\n"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"artifact bytes changed");
    assert_eq!(digest(&scratch.root), before);
    assert_eq!(
        ok(&scratch.root, "change_artifact_target", change),
        "null\n"
    );
    assert_eq!(
        std::fs::read(&target).unwrap(),
        b"artifact bytes changed changed"
    );
    assert_eq!(digest(&scratch.root), before);
    assert!(
        try_open(&scratch.root).is_ok(),
        "the root still opens for reading"
    );
}

#[test]
fn a_missing_wal_is_refused_by_the_tool_and_by_the_reader_alike() {
    let _serial = serial();
    let scratch = seeded();
    std::fs::remove_file(scratch.root.join("store.sqlite3-wal")).unwrap();
    let out = run(&scratch.root, "append_partial_line", &id(A));
    assert_eq!((out.code, out.stdout.as_str()), (Some(1), ""));
    assert!(out.stderr.contains("store.sqlite3-wal"), "{}", out.stderr);
    assert!(!scratch.root.join("store.sqlite3-wal").exists());
    let error = match try_open(&scratch.root) {
        Ok(_) => panic!("a root without -wal must not open"),
        Err(error) => error,
    };
    assert_eq!(ReadRefusal::from(&error), ReadRefusal::WalMissing);
}

#[test]
fn a_truncate_that_removes_nothing_still_expires_a_cursor_inside_the_log() {
    let _serial = serial();
    let scratch = seeded();
    let cursor = events_with(&scratch.root, A, 4, true, None)
        .unwrap()
        .newer_cursor
        .unwrap();
    truncate_task(&scratch, A, 100);
    assert_eq!(seqs(&events(&scratch.root, A)), [0, 3, 4, 5, 6, 7]);
    let error = events_with(&scratch.root, A, 50, false, Some(cursor)).unwrap_err();
    assert!(matches!(error, Error::ExpiredEventCursor), "{error:?}");
}
