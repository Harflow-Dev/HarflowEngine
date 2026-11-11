#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Event logs and launches the fixture port cannot seed, read through `harflow-ctl`: the
//! open, settling and lost-with-owner log states, a stored capture the record contradicts,
//! pending launches, and a page of 200 lines beside an oversized one.
mod ctl_reads_store_support;
use ctl_reads_store_support::*;
use hf_application::event_read::{EventRecord, RecordClass};
use hf_domain::event_read::{CaptureState, LogFacts};
use hf_storage::EventFixture;

fn line(start: u64, seq: u64, kind: &str) -> EventRecord {
    let body = format!(
        r#"{{"product":"harflow","format":"harflow.event","v":1,"seq":{seq},"kind":"{kind}","observed_at":"2026-01-01T00:00:00+00:00","text":"t{seq}"}}"#
    )
    .into_bytes();
    EventRecord {
        start,
        end: start + body.len() as u64 + 1,
        class: RecordClass::Valid,
        seq: Some(seq),
        kind: Some(kind.into()),
        body,
    }
}

fn facts(capture: CaptureState, status: &str, drained: Option<bool>, grace: bool) -> LogFacts {
    LogFacts {
        task_status: Some(status.into()),
        available: true,
        capture,
        output_drained: drained,
        eof_finished_seq: None,
        within_grace: grace,
    }
}

fn log(writer: &hf_storage::Store, n: u32, records: Vec<EventRecord>, facts: LogFacts) {
    writer
        .replace_event_fixture(
            &EventFixture {
                task: id(n),
                records,
                partial_tail: 0,
                facts,
            },
            later(),
        )
        .unwrap();
}

fn read(root: &std::path::Path, args: &[&str]) -> String {
    let out = ctl(root, args);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    text(&out)
}

/// The compact `log` object of the newest page: keys are sorted.
fn log_of(root: &std::path::Path, n: u32) -> String {
    let body = read(root, &["task-events", id(n).as_str()]);
    let start = body.find("\"log\":{").expect("a log object") + 6;
    let end = start + body[start..].find('}').unwrap() + 1;
    body[start..end].to_string()
}

fn log_line(capture: &str, final_seq: &str, state: &str, status: &str) -> String {
    format!(
        "{{\"capture_state\":\"{capture}\",\"final_seq\":{final_seq},\"output_drained\":true,\"state\":\"{state}\",\"task_status\":\"{status}\"}}"
    )
}

fn count(body: &str, needle: &str) -> usize {
    body.matches(needle).count()
}

/// A writer whose limits admit a page of 200 lines; the reader still opens with its own.
fn roomy_store(root: &std::path::Path) -> hf_storage::Store {
    let limits = hf_storage::Limits {
        max_rows: 1_000,
        max_bytes: 4 * 1024 * 1024,
    };
    let writer =
        hf_storage::Store::initialize_fixture(root, limits, later(), hf_platform::lease::shared())
            .unwrap();
    let all = hf_domain::task_read::Coverage {
        discovery_complete: true,
        ownership_complete: true,
        source_complete: true,
    };
    let empty = hf_storage::TaskFixture {
        rows: Vec::new(),
        launches: Vec::new(),
        coverage: all,
    };
    writer.replace_task_fixture(&empty, later()).unwrap();
    writer
}

fn running_store(name: &str) -> (Scratch, std::path::PathBuf, hf_storage::Store) {
    let scratch = scratch(name);
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "running", None, 1, "/r"), later())
        .unwrap();
    (scratch, root, writer)
}

#[test]
fn a_live_capture_on_a_running_task_is_an_open_log() {
    let _one = exclusive();
    let (_scratch, root, writer) = running_store("open");
    log(
        &writer,
        1,
        vec![line(0, 0, "notice")],
        facts(CaptureState::Live, "running", None, false),
    );
    drop(writer);
    assert_eq!(
        log_of(&root, 1),
        log_line("live", "null", "open", "running")
    );
}

#[test]
fn a_lost_capture_on_a_running_task_is_an_incomplete_log_that_says_so() {
    let _one = exclusive();
    let (_scratch, root, writer) = running_store("lost");
    log(
        &writer,
        1,
        vec![line(0, 0, "notice")],
        facts(CaptureState::LostWithOwner, "running", None, false),
    );
    drop(writer);
    assert_eq!(
        log_of(&root, 1),
        log_line("lost_with_owner", "null", "incomplete", "running")
    );
}

#[test]
fn a_drained_terminal_task_inside_its_grace_without_a_finish_line_is_settling() {
    let _one = exclusive();
    let scratch = scratch("settling");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let records = vec![line(0, 0, "notice")];
    log(
        &writer,
        1,
        records.clone(),
        facts(
            CaptureState::ObservedTerminal,
            "completed",
            Some(true),
            true,
        ),
    );
    assert_eq!(
        log_of(&root, 1),
        log_line("observed_terminal", "null", "settling", "completed")
    );
    log(
        &writer,
        1,
        records,
        facts(
            CaptureState::ObservedTerminal,
            "completed",
            Some(true),
            false,
        ),
    );
    assert_eq!(
        log_of(&root, 1),
        log_line("observed_terminal", "null", "incomplete", "completed"),
        "the grace is over"
    );
}

#[test]
fn the_record_overrules_a_stored_capture_that_contradicts_it() {
    let _one = exclusive();
    let scratch = scratch("contradiction");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "running", None, 1, "/r"), later())
        .unwrap();
    writer
        .add_task_fixture(&task(2, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let records = vec![line(0, 0, "notice")];
    log(
        &writer,
        1,
        records.clone(),
        facts(CaptureState::ObservedTerminal, "running", Some(true), false),
    );
    log(
        &writer,
        2,
        records,
        facts(CaptureState::Live, "completed", Some(true), false),
    );
    drop(writer);
    assert_eq!(
        log_of(&root, 1),
        log_line("unknown", "null", "unknown", "running")
    );
    assert_eq!(
        log_of(&root, 2),
        log_line("observed_terminal", "null", "incomplete", "completed")
    );
}

#[test]
fn a_stored_unknown_capture_passes_as_unknown() {
    let _one = exclusive();
    let (_scratch, root, writer) = running_store("unknown");
    log(
        &writer,
        1,
        vec![line(0, 0, "notice")],
        facts(CaptureState::Unknown, "running", None, false),
    );
    drop(writer);
    assert_eq!(
        log_of(&root, 1),
        log_line("unknown", "null", "unknown", "running")
    );
}

#[test]
fn a_pending_launch_is_task_pending_launch_for_events_and_prompts_and_unknown_for_artifacts() {
    let _one = exclusive();
    let scratch = scratch("pending");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    writer
        .replace_launch_fixture(
            &[hf_storage::LaunchRow::waiting_for_resources(id(5))],
            later(),
        )
        .unwrap();
    drop(writer);
    let pending = id(5);
    for command in ["task-events", "task-prompt"] {
        let out = ctl(&root, &[command, pending.as_str()]);
        assert_eq!(out.status.code(), Some(1), "{command}");
        let body = text(&out);
        assert!(body.contains("\"code\":\"task_pending_launch\""), "{body}");
        assert!(
            body.contains("\"data\":{\"launch_state\":\"waiting_for_resources\"}")
                && body.contains("its launch is waiting_for_resources;"),
            "{body}"
        );
    }
    let out = ctl(&root, &["task-artifacts", pending.as_str()]);
    assert_eq!(out.status.code(), Some(1));
    let body = text(&out);
    assert!(
        body.contains("\"code\":\"artifact_error\"") && body.contains("Unknown task"),
        "{body}"
    );
}

#[test]
fn two_hundred_lines_beside_an_oversized_one_are_read_in_one_page() {
    let _one = exclusive();
    let scratch = scratch("rows");
    let root = scratch.0.join("store");
    let writer = roomy_store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let mut records = vec![EventRecord {
        start: 0,
        end: 1_000_002,
        class: RecordClass::Oversized,
        seq: None,
        kind: None,
        body: Vec::new(),
    }];
    let mut start = 1_000_002;
    for seq in 0..200 {
        let record = line(start, seq, "notice");
        start = record.end;
        records.push(record);
    }
    log(
        &writer,
        1,
        records,
        facts(
            CaptureState::ObservedTerminal,
            "completed",
            Some(true),
            false,
        ),
    );
    drop(writer);
    let page = read(
        &root,
        &[
            "task-events",
            id(1).as_str(),
            "--limit",
            "200",
            "--from",
            "oldest",
        ],
    );
    assert!(
        page.contains("\"skipped\":{\"malformed\":0,\"oversized\":1}"),
        "{page}"
    );
    assert_eq!(count(&page, "\"kind\":\"notice\""), 200);
    assert!(
        page.contains("\"span\":{\"first_seq\":0,\"last_seq\":199}"),
        "{page}"
    );
    assert!(page.contains("\"has_newer\":false"), "{page}");
    let newest = read(&root, &["task-events", id(1).as_str(), "--limit", "200"]);
    assert_eq!(count(&newest, "\"kind\":\"notice\""), 200);
    assert!(
        newest.contains("\"skipped\":{\"malformed\":0,\"oversized\":0}"),
        "the oversized line lies beyond the page: {newest}"
    );
    assert!(newest.contains("\"has_older\":true"), "{newest}");
}

#[test]
fn a_page_of_one_line_beside_oversized_lines_makes_progress() {
    let _one = exclusive();
    let scratch = scratch("progress");
    let root = scratch.0.join("store");
    let writer = store(&root);
    writer
        .add_task_fixture(&task(1, "completed", Some(0), 1, "/r"), later())
        .unwrap();
    let big = |start| EventRecord {
        start,
        end: start + 1_000_002,
        class: RecordClass::Oversized,
        seq: None,
        kind: None,
        body: Vec::new(),
    };
    let first = big(0);
    let second = line(first.end, 0, "notice");
    let third = big(second.end);
    log(
        &writer,
        1,
        vec![first, second.clone(), third],
        facts(
            CaptureState::ObservedTerminal,
            "completed",
            Some(true),
            false,
        ),
    );
    drop(writer);
    let page = read(
        &root,
        &[
            "task-events",
            id(1).as_str(),
            "--limit",
            "1",
            "--from",
            "oldest",
        ],
    );
    assert!(!page.contains("\"newer_cursor\":null"), "{page}");
    assert!(page.contains("\"has_newer\":true"), "{page}");
}
