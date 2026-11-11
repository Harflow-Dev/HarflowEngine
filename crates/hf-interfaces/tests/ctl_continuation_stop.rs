#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `continuation_stop` object of `status`, `status --brief` and `status-batch`: its shape,
//! key order and reason per state, the reader-clock stamp, and the brief's copy.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::{
    ctl::{continuation_stop::StopRow, task_rows::ChainRoot},
    task_read::TaskId,
};
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::Value;

/// 2027-01-15T08:00:00 UTC.
const WHOLE: i64 = 1_800_000_000_000_000;
const STAMP: &str = "2027-01-15T08:00:00+00:00";

fn id(value: &str) -> TaskId {
    TaskId::new(value.to_owned()).unwrap()
}

fn scripted(chain_root: ChainRoot, stop: StopRow) -> Fake {
    let mut fake = fake(
        "completed",
        Some(0),
        hf_domain::ctl::caller::PidObservation::Absent,
    );
    fake.chain_root = chain_root;
    fake.stop = stop;
    fake
}

fn marker(text: &str) -> StopRow {
    StopRow::Marker {
        attribution: Some(text.to_owned()),
    }
}

fn run(fake: &Fake, command: &[&str]) -> String {
    let out = dispatch(&args(command), fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    out.stdout
}

fn status_text(fake: &Fake) -> String {
    run(fake, &["status", ID, "--json"])
}

fn brief_text(fake: &Fake) -> String {
    run(fake, &["status", ID, "--json", "--brief"])
}

fn batch_text(fake: &Fake) -> String {
    run(fake, &["status-batch", "--task-ids", ID, "--json"])
}

fn stop_of(fake: &Fake) -> Value {
    task(fake, &[])["continuation_stop"].clone()
}

/// The serialized `continuation_stop` member exactly as written (Python separators).
fn raw_member(text: &str) -> &str {
    let key = "\"continuation_stop\": ";
    let start = text.find(key).unwrap() + key.len();
    let end = start + text[start..].find('}').unwrap() + 1;
    &text[start..end]
}

fn expected(root: &str, state: &str, reason: &str, cancelled: &str, at: &str) -> String {
    format!(
        "{{\"cancelled_task_id\": {cancelled}, \"chain_root_task_id\": {root}, \"observed_at\": {at}, \"reason\": {reason}, \"state\": \"{state}\"}}"
    )
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[test]
fn not_stopped_reports_its_own_id_when_no_root_is_stored() {
    let fake = scripted(ChainRoot::Absent, StopRow::Absent);
    assert_eq!(
        raw_member(&status_text(&fake)),
        expected(&quoted(ID), "not_stopped", "null", "null", &quoted(STAMP))
    );
}

#[test]
fn a_stored_root_is_reported_for_a_chain_member() {
    let fake = scripted(ChainRoot::Valid(id(OTHER)), marker(ID));
    assert_eq!(
        raw_member(&status_text(&fake)),
        expected(
            &quoted(OTHER),
            "stopped",
            "\"chain_cancelled\"",
            &quoted(ID),
            &quoted(STAMP)
        )
    );
}

#[test]
fn stopped_without_a_usable_attribution_has_a_null_cancelled_task() {
    let fake = scripted(ChainRoot::Absent, StopRow::Marker { attribution: None });
    assert_eq!(
        raw_member(&status_text(&fake)),
        expected(
            &quoted(ID),
            "stopped",
            "\"chain_cancelled\"",
            "null",
            &quoted(STAMP)
        )
    );
    // Text that is not an identity is dropped by the projection too.
    let fake = scripted(ChainRoot::Absent, marker("not an id!"));
    assert_eq!(stop_of(&fake)["cancelled_task_id"], Value::Null);
    assert_eq!(stop_of(&fake)["state"], "stopped");
}

#[test]
fn a_fault_is_unknown_with_the_root_and_no_reason() {
    let fake = scripted(ChainRoot::Absent, StopRow::Fault);
    assert_eq!(
        raw_member(&status_text(&fake)),
        expected(&quoted(ID), "unknown", "null", "null", &quoted(STAMP))
    );
}

#[test]
fn an_invalid_stored_root_is_unknown_with_a_null_root_whatever_the_row() {
    for chain in ["not a valid id!", ""] {
        for row in [StopRow::Absent, marker(OTHER), StopRow::Fault] {
            let fake = scripted(ChainRoot::Invalid(chain.to_owned()), row);
            assert_eq!(
                raw_member(&status_text(&fake)),
                expected("null", "unknown", "null", "null", &quoted(STAMP)),
                "{chain:?}"
            );
        }
    }
}

#[test]
fn the_reason_is_set_only_for_a_stopped_chain() {
    let cases = [
        (StopRow::Absent, Value::Null),
        (StopRow::Fault, Value::Null),
        (marker(OTHER), Value::from("chain_cancelled")),
        (
            StopRow::Marker { attribution: None },
            Value::from("chain_cancelled"),
        ),
    ];
    for (row, reason) in cases {
        let fake = scripted(ChainRoot::Absent, row.clone());
        assert_eq!(stop_of(&fake)["reason"], reason, "{row:?}");
    }
}

#[test]
fn observed_at_follows_python_isoformat() {
    for (micros, text) in [
        (WHOLE, STAMP.to_owned()),
        (WHOLE + 1, "2027-01-15T08:00:00.000001+00:00".to_owned()),
        (
            WHOLE + 120_000,
            "2027-01-15T08:00:00.120000+00:00".to_owned(),
        ),
        (
            WHOLE + 999_999,
            "2027-01-15T08:00:00.999999+00:00".to_owned(),
        ),
        (0, "1970-01-01T00:00:00+00:00".to_owned()),
    ] {
        let mut fake = scripted(ChainRoot::Absent, StopRow::Absent);
        fake.now_us = micros;
        assert_eq!(stop_of(&fake)["observed_at"], text, "{micros}");
    }
}

#[test]
fn without_a_clock_status_keeps_the_object_with_a_null_stamp() {
    let mut fake = scripted(ChainRoot::Absent, marker(OTHER));
    fake.clock = false;
    let stop = stop_of(&fake);
    assert_eq!(stop["observed_at"], Value::Null);
    assert_eq!(stop["state"], "stopped");
    assert_eq!(stop["cancelled_task_id"], OTHER);
}

#[test]
fn the_brief_copies_the_status_object_exactly() {
    for (chain, row) in [
        (ChainRoot::Absent, StopRow::Absent),
        (ChainRoot::Valid(id(OTHER)), marker(ID)),
        (ChainRoot::Absent, StopRow::Fault),
        (ChainRoot::Invalid("bad id".into()), StopRow::Absent),
    ] {
        let fake = scripted(chain, row);
        assert_eq!(
            raw_member(&brief_text(&fake)),
            raw_member(&status_text(&fake))
        );
    }
}

#[test]
fn the_brief_omits_the_object_when_there_is_no_clock() {
    let mut fake = scripted(ChainRoot::Absent, marker(OTHER));
    fake.clock = false;
    let text = brief_text(&fake);
    assert!(!text.contains("continuation_stop"), "{text}");
}

#[test]
fn a_batch_row_carries_the_same_object_as_the_single_status() {
    for (chain, row) in [
        (ChainRoot::Absent, StopRow::Absent),
        (ChainRoot::Valid(id(OTHER)), marker(ID)),
        (ChainRoot::Absent, StopRow::Fault),
        (ChainRoot::Invalid(String::new()), marker(OTHER)),
    ] {
        let fake = scripted(chain, row);
        let batch: Value = serde_json::from_str(&batch_text(&fake)).unwrap();
        assert_eq!(batch["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(batch["tasks"][0]["continuation_stop"], stop_of(&fake));
        assert_eq!(
            raw_member(&batch_text(&fake)),
            raw_member(&status_text(&fake))
        );
    }
}

#[test]
fn a_batch_row_without_a_clock_has_a_null_stamp() {
    let mut fake = scripted(ChainRoot::Absent, StopRow::Absent);
    fake.clock = false;
    let batch: Value = serde_json::from_str(&batch_text(&fake)).unwrap();
    assert_eq!(
        batch["tasks"][0]["continuation_stop"]["observed_at"],
        Value::Null
    );
}

/// 2026-10-15T00:00:00 UTC, the stamp of the vendored fixtures.
const FIXTURE_US: i64 = 1_792_022_400_000_000;

/// The fixture's member with its task id and stamp swapped for the scripted ones, and the
/// separators of the compact fixture mapped to the served form.
fn fixture_member(name: &str) -> String {
    let path = format!(
        "{}/../../tests/fixtures/contracts-v1/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).unwrap();
    let key = "\"continuation_stop\":";
    let start = text.find(key).unwrap() + key.len();
    let end = start + text[start..].find('}').unwrap() + 1;
    text[start..end]
        .replace("task-continuation-stop", ID)
        .replace(',', ", ")
        .replace("\":", "\": ")
}

fn at_fixture_time(chain_root: ChainRoot, stop: StopRow) -> Fake {
    let mut fake = scripted(chain_root, stop);
    fake.now_us = FIXTURE_US;
    fake
}

#[test]
fn every_state_matches_the_vendored_fixture_member_byte_for_byte() {
    let cases = [
        ("ctl-task-status-continuation-not-stopped", StopRow::Absent),
        ("ctl-task-status-continuation-stopped", marker(ID)),
        ("ctl-task-status-continuation-unknown", StopRow::Fault),
    ];
    for (name, row) in cases {
        let fake = at_fixture_time(ChainRoot::Absent, row);
        assert_eq!(
            raw_member(&status_text(&fake)),
            fixture_member(name),
            "{name}"
        );
    }
}

#[test]
fn the_brief_and_batch_members_match_their_fixtures() {
    let fake = at_fixture_time(ChainRoot::Absent, marker(ID));
    assert_eq!(
        raw_member(&brief_text(&fake)),
        fixture_member("ctl-task-status-brief-continuation-stopped")
    );
    assert_eq!(
        raw_member(&batch_text(&fake)),
        fixture_member("ctl-task-status-batch-continuation-stop")
    );
}
