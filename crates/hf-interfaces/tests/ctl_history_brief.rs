#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The 8,192 byte bound of a brief status over a scripted store: the reference's trim order
//! first, then the objects it copies unbounded, largest first.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::caller::PidObservation;
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const BOUND: usize = 8192;

fn member() -> Value {
    let big = "\u{e9}".repeat(85);
    json!({"a": big, "b": big, "c": big, "d": big, "e": 1})
}

fn outcome() -> Value {
    let big = "\u{e9}".repeat(85);
    json!({"base_commit": big, "head_commit": big, "new_commits": [member(), member(), member()],
           "local_upstream": member(), "publication_evidence": member(), "pushed": true,
           "baseline": {"head": big, "branch": big, "provenance": big},
           "terminal": {"head": big, "branch": big, "provenance": big}})
}

fn diagnostic() -> Value {
    let big = "\u{e9}".repeat(85);
    json!({"category": big, "reason": big, "source": big, "background_tasks": [member(), member(), member()],
           "latest_result": member(), "successful_agent_result": member()})
}

fn extras(members: &[(&str, Value)]) -> &'static str {
    let text: String = members
        .iter()
        .map(|(key, value)| format!(",\"{key}\":{value}"))
        .collect();
    Box::leak(text.into_boxed_str())
}

fn scripted(members: &[(&str, Value)], lines: usize) -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.status_extra = extras(members);
    fake.history.recent = (0..lines)
        .map(|n| format!(r#"{{"kind":"notice","text":"{}{n}"}}"#, "\u{e9}".repeat(85)).into_bytes())
        .collect();
    fake
}

fn brief(fake: &Fake) -> Value {
    let out = dispatch(&args(&["status", ID, "--json", "--brief"]), fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["task"].clone()
}

/// Python's `json.dumps(value)` size: default separators and `ensure_ascii` escapes.
fn size(value: &Value) -> usize {
    match value {
        Value::Array(items) => {
            2 + items.iter().map(size).sum::<usize>() + 2 * items.len().saturating_sub(1)
        }
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, v)| size(&json!(k)) + 2 + size(v))
                .sum::<usize>()
                + 2 * map.len().saturating_sub(1)
        }
        Value::String(text) => {
            2 + text
                .chars()
                .map(|c| match c {
                    '"' | '\\' | '\n' | '\r' | '\t' => 2,
                    ' '..='~' => 1,
                    c if (c as u32) < 0x10000 => 6,
                    _ => 12,
                })
                .sum::<usize>()
        }
        other => other.to_string().len(),
    }
}

fn dropped(task: &Value) -> Vec<String> {
    task["truncated_fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn fits(task: &Value) -> bool {
    size(task) <= BOUND
}

#[test]
fn a_large_outcome_and_diagnostic_are_trimmed_in_the_references_order() {
    // Values computed by brief_status() of task_reads.py (HarflowMCP a426c5a) on this document:
    // the two long texts are halved to 32 characters, then the outcome and the diagnostic are
    // dropped, largest first, and the recent activity is kept.
    let long = "\u{e9}".repeat(600);
    let fake = scripted(
        &[
            ("outcome", outcome()),
            ("failure_diagnostic", diagnostic()),
            ("summary", json!(long)),
            ("error", json!(long)),
        ],
        3,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    let mut keys: Vec<&String> = task.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "artifact_summary",
            "backend",
            "detail_refs",
            "duration_seconds",
            "error",
            "exit_code",
            "finished_at",
            "needs_reconciliation",
            "observation_mode",
            "observed_exit",
            "persisted_status",
            "process_identity_state",
            "recent_activity",
            "recovered",
            "repo_path",
            "session_id",
            "started_at",
            "status",
            "summary",
            "task_id",
            "title",
            "truncated_fields"
        ]
    );
    assert_eq!(
        dropped(&task),
        [
            "summary",
            "error",
            "outcome",
            "failure_diagnostic",
            "task_id"
        ]
    );
    assert_eq!(task["summary"].as_str().unwrap().chars().count(), 32);
    assert_eq!(task["error"].as_str().unwrap().chars().count(), 32);
    assert_eq!(task["recent_activity"].as_array().unwrap().len(), 3);
}

fn big_text(characters: usize) -> Value {
    json!({"text": "x".repeat(characters)})
}

#[test]
fn a_huge_supervision_is_dropped_after_the_reference_optional_fields_and_the_document_fits() {
    let fake = scripted(
        &[("supervision", big_text(20_000)), ("outcome", outcome())],
        3,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    assert!(task.get("supervision").is_none());
    assert!(task.get("outcome").is_none());
    assert!(task.get("recent_activity").is_none());
    let names = dropped(&task);
    let at = |name: &str| {
        names
            .iter()
            .position(|n| n == name)
            .unwrap_or_else(|| panic!("{name} in {names:?}"))
    };
    assert!(at("outcome") < at("supervision"), "{names:?}");
    assert!(at("recent_activity") < at("supervision"), "{names:?}");
    assert!(fits(&task));
}

#[test]
fn the_larger_of_the_unbounded_objects_goes_first_and_the_smaller_stays_if_that_is_enough() {
    let fake = scripted(
        &[
            ("supervision", big_text(5_000)),
            ("successor_tasks", big_text(5_400)),
        ],
        0,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    assert!(task.get("successor_tasks").is_none());
    assert_eq!(task["supervision"], big_text(5_000));
    // The reference halves the 36 character task id and drops the empty summary list first.
    assert_eq!(
        dropped(&task),
        ["task_id", "recent_activity", "successor_tasks"]
    );
    let swapped = scripted(
        &[
            ("supervision", big_text(5_400)),
            ("successor_tasks", big_text(5_000)),
        ],
        0,
    );
    let task = brief(&swapped);
    assert!(fits(&task), "over the bound");
    assert!(task.get("supervision").is_none());
    assert_eq!(task["successor_tasks"], big_text(5_000));
}

#[test]
fn a_large_first_phase_slot_goes_before_a_smaller_supervision() {
    let slots = json!([{"text": "s".repeat(9_000)}]);
    let fake = scripted(
        &[("phase_slots", slots), ("supervision", big_text(3_000))],
        0,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    assert!(task.get("phase_slots").is_none(), "{task}");
    assert_eq!(task["supervision"], big_text(3_000));
    assert!(dropped(&task).contains(&"phase_slots".to_string()));
}

#[test]
fn the_optional_fields_of_the_reference_suffice_when_they_are_enough() {
    let fake = scripted(
        &[("supervision", big_text(6_500)), ("outcome", outcome())],
        3,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    assert_eq!(
        task["supervision"],
        big_text(6_500),
        "the unbounded object is the last resort"
    );
    assert!(task.get("outcome").is_none());
}

#[test]
fn a_document_under_the_bound_is_not_trimmed() {
    let fake = scripted(
        &[
            ("supervision", big_text(2_000)),
            ("successor_tasks", big_text(2_000)),
        ],
        3,
    );
    let task = brief(&fake);
    assert!(fits(&task), "over the bound");
    assert_eq!(task["supervision"], big_text(2_000));
    assert_eq!(task["successor_tasks"], big_text(2_000));
    assert_eq!(task["recent_activity"].as_array().unwrap().len(), 3);
    assert!(dropped(&task).is_empty(), "{:?}", dropped(&task));
}
