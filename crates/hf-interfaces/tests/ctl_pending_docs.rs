#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The documents of `task-pending-page` and `task-pending-status` against the vendored
//! conformance fixtures, byte for byte once the engine's own product version is applied.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_application::ctl::task_reads::{Namespace, NamespaceFailure};
use hf_domain::{ctl::caller::PidObservation, ctl::pending::LaunchFacts};
use hf_interfaces::ctl::dispatch::{Outcome, dispatch};
use std::{fs, path::PathBuf};

/// 2026-11-10T00:00:00Z in microseconds since the epoch: the clock of the fixtures.
const FIXTURE_CLOCK: i64 = 1_794_268_800_000_000;

fn expected(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/contracts-v1")
        .join(name);
    let text = fs::read_to_string(path).unwrap();
    text.trim_end().replace(
        "\"product_version\":\"0.0.13\"",
        "\"product_version\":\"0.0.13+rust\"",
    )
}

/// The document with the whitespace between its tokens removed.
fn compact(document: &str) -> String {
    let mut out = String::with_capacity(document.len());
    let (mut quoted, mut escaped) = (false, false);
    for ch in document.chars() {
        if quoted {
            out.push(ch);
            match (escaped, ch) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
        } else if !ch.is_whitespace() {
            quoted = ch == '"';
            out.push(ch);
        }
    }
    out
}

fn world() -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.now_us = FIXTURE_CLOCK;
    fake
}

fn launch(id: &str, state: &str) -> LaunchFacts {
    LaunchFacts {
        id: id.into(),
        state: Some(state.into()),
        generation: Some("c".repeat(32)),
        title: Some("Fixture pending launch".into()),
        backend: Some("claude".into()),
        repo_path: Some("/tmp/harflow-fixture-repo".into()),
        created_at: Some("2026-11-10T00:00:00+00:00".into()),
        text_valid: true,
        ..LaunchFacts::default()
    }
}

fn with_resources(mut facts: LaunchFacts) -> LaunchFacts {
    facts.has_resources = true;
    facts.resource_names = Some(vec!["fixture".into()]);
    facts
}

fn five() -> Vec<LaunchFacts> {
    let mut refused = with_resources(launch(&"5".repeat(32), "not_started"));
    refused.not_started_reason = Some("Fixture pre-spawn refusal".into());
    vec![
        launch(&"1".repeat(32), "reserved"),
        launch(&"2".repeat(32), "claimed"),
        with_resources(launch(&"3".repeat(32), "waiting_for_resources")),
        with_resources(launch(&"4".repeat(32), "granted")),
        refused,
    ]
}

fn revision(fake: &mut Fake, revision: &str, invalid: u64) {
    fake.pending.namespace = Ok(Namespace {
        revision: revision.into(),
        invalid,
    });
}

fn run(fake: &Fake, command: &[&str]) -> Outcome {
    let mut list = command.to_vec();
    list.push("--json");
    dispatch(&args(&list), fake)
}

fn assert_document(out: &Outcome, name: &str, code: i32) {
    assert_eq!(out.code, code, "{}", out.stderr);
    assert_eq!(compact(out.stdout.trim_end()), expected(name), "{name}");
}

#[test]
fn the_empty_namespace_page() {
    let mut fake = world();
    revision(
        &mut fake,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        0,
    );
    let out = run(&fake, &["task-pending-page"]);
    assert_document(&out, "ctl-task-pending-empty.json", 0);
}

#[test]
fn a_page_of_every_launch_state() {
    let mut fake = world();
    fake.pending.rows = five();
    revision(
        &mut fake,
        "5afdf7a4818dd654dcebc5d5d2213844a9aaff9fff542f96cbd986c1157a4563",
        0,
    );
    let out = run(&fake, &["task-pending-page"]);
    assert_document(&out, "ctl-task-pending-states.json", 0);
}

#[test]
fn a_first_page_carries_the_cursor_of_the_fixture() {
    let mut fake = world();
    fake.pending.rows = five();
    revision(
        &mut fake,
        "5afdf7a4818dd654dcebc5d5d2213844a9aaff9fff542f96cbd986c1157a4563",
        0,
    );
    let mut list = args(&["task-pending-page", "--limit", "2", "--json"]);
    let first = dispatch(&list, &fake);
    let value: serde_json::Value = serde_json::from_str(&first.stdout).unwrap();
    let cursor = value["result"]["next_cursor"].as_str().unwrap().to_owned();
    list = args(&[
        "task-pending-page",
        "--limit",
        "2",
        "--cursor",
        &cursor,
        "--json",
    ]);
    let second = dispatch(&list, &fake);
    assert_eq!(second.code, 0, "{}", second.stderr);
    let items =
        serde_json::from_str::<serde_json::Value>(&second.stdout).unwrap()["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["task_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
    assert_eq!(items, ["3".repeat(32), "4".repeat(32)]);
    // The second page of the fixture is the walk after the first two rows.
    assert_document(&second, "ctl-task-pending-next-page.json", 0);
}

#[test]
fn a_refused_receipt_and_an_invalid_name_make_the_page_unknown() {
    let mut fake = world();
    let a = "a".repeat(32);
    let b = "b".repeat(32);
    let refused = LaunchFacts {
        id: b,
        refusal: Some(("format".into(), "format".into())),
        ..LaunchFacts::default()
    };
    fake.pending.rows = vec![launch(&a, "reserved"), refused];
    revision(
        &mut fake,
        "316df6555574fb24352aca5749b902c8e3340cc96e163181aa8fac98d1952f8d",
        1,
    );
    assert_document(
        &run(&fake, &["task-pending-page"]),
        "ctl-task-pending-refused.json",
        0,
    );
}

#[test]
fn the_namespace_bound_error() {
    let mut fake = world();
    fake.pending.namespace = Err(NamespaceFailure::Bound);
    let out = run(&fake, &["task-pending-page"]);
    assert_document(&out, "ctl-task-pending-bound.json", 1);
}

#[test]
fn a_malformed_cursor_error() {
    let fake = world();
    let out = run(&fake, &["task-pending-page", "--cursor", "not-a-cursor"]);
    assert_document(&out, "ctl-task-pending-cursor-invalid.json", 1);
}

fn status(fake: &Fake, id: &str) -> Outcome {
    run(fake, &["task-pending-status", "--task-id", id])
}

#[test]
fn the_status_of_each_disposition() {
    let a = "a".repeat(32);
    let mut fake = world();
    assert_document(
        &status(&fake, &a),
        "ctl-task-pending-status-not-found.json",
        0,
    );
    fake.pending.rows = vec![launch(&a, "reserved")];
    assert_document(
        &status(&fake, &a),
        "ctl-task-pending-status-pending.json",
        0,
    );
    fake.pending.rows[0].record = true;
    assert_document(
        &status(&fake, &a),
        "ctl-task-pending-status-recorded.json",
        0,
    );
    fake.pending.rows[0] = LaunchFacts {
        id: a.clone(),
        refusal: Some(("format".into(), "format".into())),
        ..LaunchFacts::default()
    };
    assert_document(
        &status(&fake, &a),
        "ctl-task-pending-status-refused.json",
        0,
    );
    fake.pending.rows[0] = launch(&a, "spawned");
    assert_document(
        &status(&fake, &a),
        "ctl-task-pending-status-unknown.json",
        0,
    );
}

#[test]
fn without_a_clock_observed_at_is_null_in_both_documents() {
    let mut fake = world();
    fake.clock = false;
    fake.pending.rows = vec![launch(&"a".repeat(32), "reserved")];
    let page = compact(&run(&fake, &["task-pending-page"]).stdout);
    assert!(page.contains("\"observed_at\":null"), "{page}");
    let status = compact(&status(&fake, &"a".repeat(32)).stdout);
    assert!(status.contains("\"observed_at\":null"), "{status}");
    fake.clock = true;
    let with = compact(&run(&fake, &["task-pending-page"]).stdout);
    assert!(
        with.contains("\"observed_at\":\"2026-11-10T00:00:00+00:00\""),
        "{with}"
    );
}

#[test]
fn the_plain_form_prints_the_bare_result_without_the_envelope() {
    let mut fake = world();
    fake.pending.rows = vec![launch(&"a".repeat(32), "reserved")];
    let out = dispatch(&args(&["task-pending-page"]), &fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert!(
        value.get("result").is_none() && value.get("format").is_none(),
        "{value}"
    );
    assert_eq!(value["scope"], "prerecord_launch_attempts_and_not_started");
    assert!(
        out.stdout.contains('\n') && out.stdout.lines().count() > 1,
        "indented"
    );
    let out = dispatch(
        &args(&["task-pending-status", "--task-id", &"a".repeat(32)]),
        &fake,
    );
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(value["disposition"], "pending");
}

#[test]
fn rows_are_listed_in_filename_order_whatever_the_store_order() {
    let mut fake = world();
    fake.pending.rows = vec![
        launch(&"b".repeat(32), "reserved"),
        launch(&"a".repeat(32), "reserved"),
    ];
    let out = dispatch(&args(&["task-pending-page", "--json"]), &fake);
    let value: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let ids: Vec<&str> = value["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["task_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["a".repeat(32), "b".repeat(32)]);
}
