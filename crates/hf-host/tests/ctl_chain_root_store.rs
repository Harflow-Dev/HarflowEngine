#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! An invalid stored chain root through the binary: only the related-header traversal of
//! `task-list-page` fails on it; every other read succeeds and echoes the raw text.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_storage::EventFixture;
use std::path::Path;

const INVALID: &str = "not a valid id!";

fn events() -> EventFixture {
    EventFixture {
        task: id(2),
        records: Vec::new(),
        partial_tail: 0,
        facts: LogFacts {
            task_status: Some("completed".into()),
            available: true,
            capture: CaptureState::Live,
            output_drained: None,
            eof_finished_seq: None,
            within_grace: false,
        },
    }
}

fn long() -> String {
    "a".repeat(70)
}

/// Tasks 1 (root), 2 (the record under test, a continuation of 1) and 3 (unrelated).
fn root_with(scratch: &Scratch, name: &str, chain_root: Option<&str>) -> std::path::PathBuf {
    let root = scratch.0.join(name);
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let mut second = task(2, "completed", Some(0), 1, "/r");
    second.facts.continuation_of = Some(id(1));
    second.facts.chain_root = chain_root.map(str::to_owned);
    writer.add_task_fixture(&second, later()).unwrap();
    writer
        .add_task_fixture(&task(3, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer.replace_event_fixture(&events(), later()).unwrap();
    root
}

fn quoted(value: &str) -> String {
    format!("\"chain_root_task_id\":\"{value}\"")
}

fn run(root: &Path, args: &[&str]) -> (Option<i32>, String) {
    let out = ctl(root, args);
    (out.status.code(), text(&out))
}

#[test]
fn status_brief_batch_and_list_succeed_and_echo_the_raw_invalid_root() {
    let _one = exclusive();
    let scratch = scratch("chain-status");
    for (name, value) in [
        ("malformed", INVALID.to_owned()),
        ("overlong", long()),
        ("empty", String::new()),
    ] {
        let root = root_with(&scratch, name, Some(&value));
        let two = id(2);
        for args in [
            vec!["status", two.as_str()],
            vec!["status-batch", "--task-ids", two.as_str()],
            vec!["list"],
        ] {
            let (code, body) = run(&root, &args);
            assert_eq!(code, Some(0), "{name} {args:?}: {body}");
            assert!(body.contains(&quoted(&value)), "{name} {args:?}: {body}");
        }
        let (code, body) = run(&root, &["status", two.as_str(), "--brief"]);
        assert_eq!(code, Some(0), "{name} brief: {body}");
    }
}

#[test]
fn an_absent_root_reads_as_null() {
    let _one = exclusive();
    let scratch = scratch("chain-absent");
    let root = root_with(&scratch, "absent", None);
    let (code, body) = run(&root, &["status", id(2).as_str()]);
    assert_eq!(code, Some(0), "{body}");
    assert!(body.contains("\"chain_root_task_id\":null"), "{body}");
}

#[test]
fn statistics_page_echoes_the_raw_root_and_stays_complete() {
    let _one = exclusive();
    let scratch = scratch("chain-stats");
    for (name, value) in [("malformed", INVALID.to_owned()), ("empty", String::new())] {
        let root = root_with(&scratch, name, Some(&value));
        let (code, body) = run(
            &root,
            &[
                "statistics-page",
                "--since",
                "2026-01-01T00:00:00+00:00",
                "--before",
                "2026-02-01T00:00:00+00:00",
            ],
        );
        assert_eq!(code, Some(0), "{name}: {body}");
        assert!(body.contains(&quoted(&value)), "{name}: {body}");
        assert!(body.contains("\"history_complete\":true"), "{name}: {body}");
        assert!(body.contains("\"diagnostics\":[]"), "{name}: {body}");
    }
}

#[test]
fn task_events_and_the_interaction_feed_do_not_involve_the_root() {
    let _one = exclusive();
    let scratch = scratch("chain-feeds");
    let root = root_with(&scratch, "feeds", Some(INVALID));
    for args in [
        vec!["task-events", id(2).as_str()],
        vec!["interaction-events"],
    ] {
        let (code, body) = run(&root, &args);
        assert_eq!(code, Some(0), "{args:?}: {body}");
    }
}

fn invalid_params(root: &Path, args: &[&str], raw: &str) {
    let (code, body) = run(root, args);
    assert_eq!(code, Some(1), "{args:?}: {body}");
    assert!(body.contains("\"code\":\"invalid_params\""), "{body}");
    assert!(
        body.contains(&format!("\"message\":\"not a valid task id: '{raw}'\"")),
        "{body}"
    );
}

#[test]
fn a_page_that_reaches_the_invalid_link_fails_with_invalid_params() {
    let _one = exclusive();
    let scratch = scratch("chain-page");
    for (name, value) in [("malformed", INVALID.to_owned()), ("overlong", long())] {
        let root = root_with(&scratch, name, Some(&value));
        let two = id(2);
        let one = id(1);
        invalid_params(&root, &["task-list-page"], &value);
        invalid_params(
            &root,
            &["task-list-page", "--task-ids", two.as_str()],
            &value,
        );
        invalid_params(
            &root,
            &["task-list-page", "--predecessor-task-id", one.as_str()],
            &value,
        );
    }
}

#[test]
fn a_page_not_containing_the_task_succeeds_and_so_does_a_predecessor_without_successors() {
    let _one = exclusive();
    let scratch = scratch("chain-skip");
    let root = root_with(&scratch, "skip", Some(INVALID));
    let (code, body) = run(&root, &["task-list-page", "--task-ids", id(3).as_str()]);
    assert_eq!(code, Some(0), "{body}");
    assert!(body.contains(id(3).as_str()), "{body}");
    let (code, body) = run(
        &root,
        &["task-list-page", "--predecessor-task-id", id(2).as_str()],
    );
    assert_eq!(code, Some(0), "{body}");
}

#[test]
fn paging_fails_only_on_the_cursor_page_that_reaches_the_task() {
    let _one = exclusive();
    let scratch = scratch("chain-cursor");
    let root = root_with(&scratch, "cursor", Some(INVALID));
    let (code, body) = run(&root, &["task-list-page", "--limit", "1"]);
    assert_eq!(code, Some(0), "{body}");
    assert!(
        body.contains(id(3).as_str()) && !body.contains(id(2).as_str()),
        "{body}"
    );
    let key = "\"next_cursor\":\"";
    let at = body.find(key).expect("a next cursor") + key.len();
    let cursor = &body[at..][..body[at..].find('"').unwrap()];
    invalid_params(
        &root,
        &["task-list-page", "--limit", "1", "--cursor", cursor],
        INVALID,
    );
}

#[test]
fn an_empty_stored_root_is_skipped_by_the_traversal_and_an_absent_one_adds_no_link() {
    let _one = exclusive();
    let scratch = scratch("chain-empty");
    for (name, value) in [("empty", Some("")), ("absent", None)] {
        let root = root_with(&scratch, name, value);
        let (code, body) = run(&root, &["task-list-page", "--task-ids", id(2).as_str()]);
        assert_eq!(code, Some(0), "{name}: {body}");
        assert!(body.contains(id(2).as_str()), "{name}: {body}");
        let (code, body) = run(&root, &["task-list-page"]);
        assert_eq!(code, Some(0), "{name}: {body}");
    }
}
