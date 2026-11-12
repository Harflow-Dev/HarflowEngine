#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Ordinary reads of an id whose launch receipt does not qualify as a launch document: the
//! stored refusal code and a message naming the receipt, with no `data`, never `unknown_task`.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::caller::PidObservation;
use hf_interfaces::ctl::dispatch::{Outcome, dispatch};
use serde_json::Value;

fn world(code: &'static str, check: &'static str) -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.launch_refusal = Some((code, check));
    fake
}

fn run(fake: &Fake, list: &[&str]) -> Outcome {
    let mut list = list.to_vec();
    list.push("--json");
    dispatch(&args(&list), fake)
}

fn error_of(out: &Outcome) -> Value {
    serde_json::from_str::<Value>(&out.stdout).unwrap()["error"].clone()
}

fn message(check: &str) -> String {
    format!("/s/tasks/{ID}.launch.json [{check}]: ")
}

#[test]
fn status_reports_the_stored_code_and_the_receipt_without_data() {
    let out = run(&world("oversize", "oversize"), &["status", ID]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    let error = error_of(&out);
    assert_eq!(error["code"], "oversize");
    assert_eq!(
        error["message"],
        format!("{}Task launch exceeds 65536 bytes", message("oversize"))
    );
    assert!(error.get("data").is_none(), "{error}");
    assert_eq!(
        out.stderr,
        format!(
            "harflow-ctl: error: {}\n",
            error["message"].as_str().unwrap()
        )
    );
}

#[test]
fn each_check_has_its_reason_text() {
    for (code, check, reason) in [
        ("format", "format", "Incompatible task launch format"),
        ("version", "version", "Incompatible task launch version"),
        (
            "root_mismatch",
            "root_mismatch",
            "Task launch root differs from selected root",
        ),
        ("generation", "generation", "Invalid launch generation"),
        ("field_shape", "field_shape", "Malformed task launch field"),
        (
            "field_shape",
            "resource_names",
            "Malformed task launch field",
        ),
        (
            "field_shape",
            "display_text_encoding",
            "Malformed task launch field",
        ),
        (
            "task_unavailable",
            "source_changed",
            "Task launch changed while reading",
        ),
    ] {
        let out = run(&world(code, check), &["status", ID]);
        let error = error_of(&out);
        assert_eq!(error["code"], code, "{check}");
        assert_eq!(
            error["message"],
            format!("{}{reason}", message(check)),
            "{check}"
        );
    }
}

#[test]
fn it_is_no_longer_an_unknown_task() {
    let out = run(&world("format", "format"), &["status", ID]);
    assert_ne!(error_of(&out)["code"], "unknown_task");
    assert!(!out.stdout.contains("unknown_task"), "{}", out.stdout);
}

#[test]
fn status_batch_reports_the_document_error_for_the_id() {
    let fake = world("version", "version");
    let out = run(&fake, &["status-batch", "--task-ids", ID]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let value: Value = serde_json::from_str(&out.stdout).unwrap();
    let error = &value["errors"][ID];
    assert_eq!(error["code"], "version", "{value}");
    assert_eq!(
        error["message"],
        format!("{}Incompatible task launch version", message("version"))
    );
    assert!(error.get("data").is_none(), "{error}");
    assert_eq!(value["tasks"], serde_json::json!([]));
}

#[test]
fn task_events_reports_the_document_error() {
    let out = run(&world("generation", "generation"), &["task-events", ID]);
    assert_eq!(out.code, 1, "{}", out.stdout);
    let error = error_of(&out);
    assert_eq!(error["code"], "generation");
    assert_eq!(
        error["message"],
        format!("{}Invalid launch generation", message("generation"))
    );
    assert!(error.get("data").is_none(), "{error}");
}
