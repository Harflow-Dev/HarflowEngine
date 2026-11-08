#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The documents of recovery-activity, interaction-events and statistics-page as the transport
//! writes them. Expected values are the reference engine's (`project()` of statistics.py and
//! the rows of interaction_events.py, HarflowMCP a426c5a, python3 3.13.5), pasted in.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_application::ctl::task_reads::ScanFailure;
use hf_domain::{
    ctl::{caller::PidObservation, feed::FeedSource},
    event_read::CaptureState,
};
use hf_interfaces::ctl::dispatch::{Outcome, dispatch};
use serde_json::{Value, json};

const FIRST: i64 = 1_768_039_200_000_000;
const WINDOW: [&str; 4] = [
    "--since",
    "2026-01-01T00:00:00Z",
    "--before",
    "2027-01-01T00:00:00Z",
];

fn base() -> Fake {
    fake("running", None, PidObservation::Absent)
}

fn stats(fake: &Fake, extra: &[&str]) -> Outcome {
    let mut list = vec!["statistics-page", "--json"];
    list.extend(WINDOW);
    list.extend(extra);
    dispatch(&args(&list), fake)
}

fn result(out: &Outcome) -> Value {
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["result"].clone()
}

fn item_a() -> Value {
    json!({
        "accounting": {"additive": false, "categories": {"cache_creation_tokens": null, "cache_read_tokens": null,
            "input_tokens": null, "output_tokens": null}, "contribution_id": "task:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "diagnostics": ["legacy_missing_accounting"], "qualified_wait_intervals": [], "scope": "unknown",
            "state": "unavailable", "total_tokens": null, "version": 1},
        "activation_id": null, "chain_root_task_id": null, "cohort_started_at": 1768039200.0,
        "continuation_of_task_id": null, "cost_additive": false, "cost_provenance": null, "cost_usd": null,
        "duration_additive": true, "elapsed_seconds": 300.0, "execution_kind": "initial", "execution_seconds": 300.0,
        "finished_at": 1768039500.0, "id": "execution:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "kind": "execution",
        "node_id": null, "observed_model": null, "observed_model_provenance": null, "owner_task_id": null,
        "parent_task_id": null, "repo_path": "/workspace/repo", "requested_harness": "claude",
        "requested_model": "model-a", "root_workflow_run_id": null, "started_at": 1768039200.0, "status": "completed",
        "task_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "time_qualification": "elapsed_may_include_waits",
        "title": "First task", "usage_additive": false, "wait_seconds": 0, "workflow_id": null, "workflow_name": null,
        "workflow_revision": null, "workflow_run_id": null,
    })
}

#[test]
fn a_task_is_one_execution_item_of_35_keys() {
    let mut fake = base();
    fake.feed.rows = vec![hf_application::ctl::task_reads::ScanRow::Facts(Box::new(
        facts(ID, FIRST, FIRST + 300_000_000),
    ))];
    let out = stats(&fake, &[]);
    let page = result(&out);
    assert_eq!(page["items"][0], item_a());
    assert_eq!(page["items"][0].as_object().unwrap().len(), 35);
    assert!(
        out.stdout.contains("\"wait_seconds\": 0,"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("\"elapsed_seconds\": 300.0,"));
    assert_eq!(page["observed_at"], "2027-01-15T08:00:00+00:00");
    assert_eq!(page["snapshot_id"], page["generation"]);
    assert_eq!(
        page["accounting_coverage"],
        json!({"eligible_executions": 1, "available_executions": 0})
    );
    assert_eq!(page["cost_coverage"], page["accounting_coverage"]);
    assert_eq!(page["retention"], json!({"state": "days", "days": 30}));
}

#[test]
fn floats_are_written_as_python_repr_writes_them() {
    // (start, end) in microseconds -> repr of the start, the end and their difference.
    let cases: [(i64, i64, &str, &str, &str); 6] = [
        (1, 2, "1e-06", "2e-06", "1e-06"),
        (10, 20, "1e-05", "2e-05", "1e-05"),
        (15, 30, "1.5e-05", "3e-05", "1.5e-05"),
        (0, 100, "0.0", "0.0001", "0.0001"),
        (
            1_768_039_200_000_001,
            1_768_039_200_000_002,
            "1768039200.000001",
            "1768039200.000002",
            "9.5367431640625e-07",
        ),
        (-1, 0, "-1e-06", "0.0", "1e-06"),
    ];
    for (start, end, first, last, elapsed) in cases {
        let mut fake = base();
        fake.feed.rows = vec![hf_application::ctl::task_reads::ScanRow::Facts(Box::new(
            facts(ID, start, end),
        ))];
        let out = dispatch(
            &args(&[
                "statistics-page",
                "--json",
                "--since",
                "0001-01-01T00:00:00+14:00",
                "--before",
                "9999-12-31T00:00:00Z",
            ]),
            &fake,
        );
        for (key, text) in [
            ("started_at", first),
            ("finished_at", last),
            ("elapsed_seconds", elapsed),
        ] {
            assert!(
                out.stdout.contains(&format!("\"{key}\": {text},")),
                "{key} {text}: {}",
                out.stdout
            );
        }
    }
}

#[test]
fn the_unavailable_document_has_the_reference_base_shape() {
    let mut fake = base();
    fake.feed.scan_failure = Some(ScanFailure::Deadline);
    let page = result(&stats(&fake, &[]));
    let mut expected = json!({
        "projection_version": 1, "state": "unavailable", "snapshot_id": null, "generation": null,
        "items": [], "next_cursor": null, "has_more": false, "history_complete": false, "ownership_complete": false,
        "accounting_coverage": {"eligible_executions": 0, "available_executions": 0},
        "cost_coverage": {"eligible_executions": 0, "available_executions": 0},
        "retained_history_only": true, "retention": {"state": "days", "days": 30},
        "diagnostics": [{"code": "discovery_budget", "message": "No source classification fits the foreground deadline"}],
    });
    expected["observed_at"] = page["observed_at"].clone();
    assert_eq!(page, expected);
}

#[test]
fn without_json_the_bare_result_is_indented_by_two() {
    let mut fake = base();
    fake.feed.rows = vec![hf_application::ctl::task_reads::ScanRow::Facts(Box::new(
        facts(ID, FIRST, FIRST + 300_000_000),
    ))];
    let mut list = vec!["statistics-page"];
    list.extend(WINDOW);
    let out = dispatch(&args(&list), &fake);
    assert_eq!(out.code, 0);
    assert!(
        out.stdout
            .starts_with("{\n  \"accounting_coverage\": {\n    \"available_executions\": 0,"),
        "{}",
        &out.stdout[..60]
    );
    let document: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(document["items"][0], item_a());
    assert!(out.stdout.contains("\"elapsed_seconds\": 300.0"));
}

fn done(id: &str, finished: i64) -> FeedSource {
    FeedSource {
        id: id.into(),
        status: "completed".into(),
        exit_code: Some(0),
        title: Some("T".into()),
        started_at_us: 0,
        finished_at_us: Some(finished),
        metadata_bytes: 1,
    }
}

#[test]
fn an_interaction_event_row_has_the_reference_keys_and_python_floats() {
    let mut fake = base();
    fake.feed.sources = vec![
        done("t1", 1_768_039_500_000_000),
        done("t2", 1),
        done("t3", 15),
    ];
    let out = dispatch(&args(&["interaction-events", "--json"]), &fake);
    let page = result(&out);
    assert_eq!(
        page["events"][0],
        json!({
            "event_id": "task:t1:completed", "interaction_id": "t1", "kind": "task", "state": "completed",
            "task_id": "t1", "workflow_run_id": null, "source_workflow_run_id": null, "title": "T",
            "occurred_at": 1768039500.0,
        })
    );
    assert!(
        out.stdout.contains("\"occurred_at\": 1e-06,"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("\"occurred_at\": 1.5e-05,"));
    assert!(out.stdout.contains("\"occurred_at\": 1768039500.0,"));
    assert_eq!(
        (page["cycle_complete"].clone(), page["next_cursor"].clone()),
        (json!(true), Value::Null)
    );
    let bare = dispatch(&args(&["interaction-events"]), &fake);
    assert!(
        bare.stdout.starts_with("{\n  \"cycle_complete\": true,"),
        "{}",
        bare.stdout
    );
    assert!(bare.stdout.contains("\"occurred_at\": 1e-06"));
}

#[test]
fn the_cursor_issued_for_a_full_page_decodes_to_the_documented_state() {
    use base64::{Engine, engine::general_purpose::URL_SAFE};
    let mut fake = base();
    fake.feed.sources = vec![done("t1", 1), done("t2", 2), done("t3", 3)];
    let page = result(&dispatch(
        &args(&["interaction-events", "--json", "--limit", "2"]),
        &fake,
    ));
    let text = page["next_cursor"].as_str().unwrap();
    let decoded: Value = serde_json::from_slice(&URL_SAFE.decode(text).unwrap()).unwrap();
    assert_eq!(
        decoded,
        json!({"v": 1, "phase": "task", "after": "t2", "source": "t3", "event": ""})
    );
    assert_eq!(page["cycle_complete"], false);
}

#[test]
fn the_recovery_page_is_empty_and_its_notice_follows_the_capture() {
    let cases = [
        (None, "Transcript recovery requires confirmed capture loss"),
        (
            Some(CaptureState::Live),
            "Transcript recovery requires confirmed capture loss",
        ),
        (
            Some(CaptureState::LostWithOwner),
            "Transcript recovery is not available in this engine",
        ),
    ];
    for (capture, notice) in cases {
        let mut fake = base();
        if let Some(capture) = capture {
            fake.feed.captures.insert(ID.to_string(), capture);
        }
        let out = dispatch(&args(&["recovery-activity", ID]), &fake);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout.matches('\n').count(), 1);
        let document: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(
            document["recovery_activity"],
            json!({
                "source": "backend_transcript", "display_only": true, "events": [], "next_cursor": null,
                "has_more": false, "reset": false, "notices": [notice],
            })
        );
        assert_eq!(
            (document["task_id"].clone(), document["v"].clone()),
            (json!(ID), json!(1))
        );
        assert_eq!(
            dispatch(&args(&["recovery-activity", ID, "--json"]), &fake).stdout,
            out.stdout
        );
    }
}

#[test]
fn retention_is_the_environment_read_as_int_reads_it() {
    for (raw, expected) in [
        (None, json!({"state": "days", "days": 30})),
        (Some("7"), json!({"state": "days", "days": 7})),
        (Some(" 007 "), json!({"state": "days", "days": 7})),
        (Some("0"), json!({"state": "disabled", "days": null})),
        (Some("-3"), json!({"state": "unknown", "days": null})),
        (Some("x"), json!({"state": "unknown", "days": null})),
        (Some("\u{0665}"), json!({"state": "days", "days": 5})),
    ] {
        let mut fake = base();
        fake.retention = raw.map(str::to_string);
        assert_eq!(result(&stats(&fake, &[]))["retention"], expected, "{raw:?}");
    }
    let mut fake = base();
    fake.retention = Some("99999999999999999999999".into());
    assert!(
        stats(&fake, &[])
            .stdout
            .contains("\"days\": 99999999999999999999999,")
    );
}
