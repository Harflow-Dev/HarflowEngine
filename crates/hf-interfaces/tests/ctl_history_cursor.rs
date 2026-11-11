#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `data.reason` of an artifact cursor failure, the cursor decoding and spelling, the
//! page of a task without a manifest and the plan provenance of an artifact entry.
mod ctl_reads_support;
use base64::{Engine, engine::general_purpose::URL_SAFE};
use ctl_reads_support::*;
use hf_domain::ctl::{
    caller::PidObservation,
    history::{ArtifactItem, ManifestHead, Observation, PlanDirectory, PlanEvidence},
};
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const REVISION: &str = "0123456789abcdef0123456789abcdef";
const EMPTY_REVISION: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const MESSAGE: &str = "Invalid or stale artifact cursor";

fn item(number: u64, plan: Option<PlanEvidence>) -> ArtifactItem {
    ArtifactItem {
        artifact_id: format!("x{number}"),
        path: format!("/repo/out/f{number}.txt"),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-01T00:00:00+00:00".into(),
        method: if plan.is_some() {
            "observed_backend_plan".into()
        } else {
            "verified_process_lineage".into()
        },
        plan,
        registered: Observation {
            device: 1,
            inode: number,
            size: 3,
            mtime_ns: 4,
        },
    }
}

fn with_items(count: u64) -> Fake {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.history.manifest = Some(ManifestHead {
        header_valid: true,
        revision: REVISION.into(),
        count: count as usize,
    });
    fake.history.items = (0..count).map(|n| item(n, None)).collect();
    fake
}

fn run(fake: &Fake, id: &str, extra: &[&str]) -> (i32, Value, String) {
    let mut list = vec!["task-artifacts", id, "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), fake);
    (
        out.code,
        serde_json::from_str(&out.stdout).unwrap_or(Value::Null),
        out.stdout,
    )
}

/// A cursor spelled from raw JSON text, so integers beyond `serde_json`'s range can be written.
fn raw_cursor(task: &str, revision: &str, offset: &str) -> String {
    URL_SAFE.encode(format!("[\"{task}\", \"{revision}\", {offset}]"))
}

fn reason_of(fake: &Fake, cursor: &str) -> Option<String> {
    let (code, doc, _) = run(fake, ID, &["--cursor", cursor]);
    assert_eq!(code, 1, "{cursor}");
    assert_eq!(doc["error"]["code"], "artifact_error", "{cursor}");
    assert_eq!(doc["error"]["message"], MESSAGE, "{cursor}");
    doc["error"]["data"]["reason"].as_str().map(str::to_owned)
}

#[test]
fn a_malformed_cursor_is_invalid_even_when_it_would_also_be_stale() {
    let fake = with_items(5);
    for (label, cursor) in [
        (
            "negative offset",
            raw_cursor(ID, "f".repeat(32).as_str(), "-1"),
        ),
        ("negative, other task", raw_cursor(OTHER, REVISION, "-1")),
        (
            "negative offset, current revision",
            raw_cursor(ID, REVISION, "-1"),
        ),
        (
            "text offset",
            raw_cursor(ID, "f".repeat(32).as_str(), "\"1\""),
        ),
        ("not base64", "!!".to_string()),
        ("empty", String::new()),
    ] {
        assert_eq!(
            reason_of(&fake, &cursor).as_deref(),
            Some("cursor_invalid"),
            "{label}"
        );
    }
}

#[test]
fn an_integer_of_any_size_is_a_valid_offset_and_the_binding_is_checked_first() {
    let fake = with_items(5);
    let other = "f".repeat(32);
    for offset in ["18446744073709551616", "99999999999999999999999"] {
        assert_eq!(
            reason_of(&fake, &raw_cursor(ID, &other, offset)).as_deref(),
            Some("cursor_stale"),
            "{offset}"
        );
        assert_eq!(
            reason_of(&fake, &raw_cursor(ID, REVISION, offset)).as_deref(),
            Some("cursor_invalid"),
            "{offset}"
        );
    }
    let (code, doc, _) = run(&fake, ID, &["--cursor", &raw_cursor(ID, REVISION, "-0")]);
    assert_eq!(code, 0);
    assert_eq!(doc["result"]["items"][0]["artifact_id"], "x0");
}

#[test]
fn a_cursor_bound_to_another_owner_or_revision_is_stale_whatever_its_offset() {
    let fake = with_items(5);
    let other = "f".repeat(32);
    for (label, cursor) in [
        ("other revision", raw_cursor(ID, &other, "1")),
        ("other revision, beyond count", raw_cursor(ID, &other, "99")),
        (
            "other revision, u64::MAX",
            raw_cursor(ID, &other, "18446744073709551615"),
        ),
        (
            "other revision, above i64",
            raw_cursor(ID, &other, "9223372036854775808"),
        ),
        ("other task", raw_cursor(OTHER, REVISION, "1")),
        (
            "other task, beyond count",
            raw_cursor(OTHER, REVISION, "99"),
        ),
    ] {
        assert_eq!(
            reason_of(&fake, &cursor).as_deref(),
            Some("cursor_stale"),
            "{label}"
        );
    }
}

#[test]
fn an_offset_beyond_the_current_manifest_is_invalid_and_the_count_is_the_end_page() {
    let fake = with_items(5);
    for offset in [
        "6",
        "9223372036854775807",
        "9223372036854775808",
        "18446744073709551615",
    ] {
        let cursor = raw_cursor(ID, REVISION, offset);
        assert_eq!(
            reason_of(&fake, &cursor).as_deref(),
            Some("cursor_invalid"),
            "{offset}"
        );
    }
    let (code, doc, _) = run(&fake, ID, &["--cursor", &raw_cursor(ID, REVISION, "5")]);
    assert_eq!(code, 0);
    assert_eq!(doc["result"]["items"], json!([]));
    assert_eq!(doc["result"]["has_more"], false);
}

#[test]
fn limit_and_task_id_errors_carry_no_data() {
    let fake = with_items(1);
    for limit in ["0", "51"] {
        let (code, doc, _) = run(&fake, ID, &["--limit", limit]);
        assert_eq!(code, 1);
        assert_eq!(doc["error"]["code"], "artifact_error");
        assert!(doc["error"].get("data").is_none(), "{doc}");
    }
    let (code, doc, _) = run(&fake, "bad/id", &[]);
    assert_eq!(code, 1);
    assert!(doc["error"].get("data").is_none(), "{doc}");
}

#[test]
fn a_task_without_a_manifest_has_the_revision_of_an_empty_file() {
    let mut fake = with_items(0);
    fake.history.manifest = None;
    let (code, doc, _) = run(&fake, ID, &[]);
    assert_eq!(code, 0);
    assert_eq!(doc["result"]["state"], "available");
    assert_eq!(doc["result"]["count"], 0);
    assert_eq!(doc["result"]["items"], json!([]));
    assert_eq!(doc["result"]["next_cursor"], Value::Null);
    let (code, doc, _) = run(
        &fake,
        ID,
        &["--cursor", &raw_cursor(ID, EMPTY_REVISION, "0")],
    );
    assert_eq!(
        (code, &doc["result"]["items"], &doc["result"]["has_more"]),
        (0, &json!([]), &json!(false))
    );
    for offset in ["1", "18446744073709551615"] {
        let cursor = raw_cursor(ID, EMPTY_REVISION, offset);
        assert_eq!(
            reason_of(&fake, &cursor).as_deref(),
            Some("cursor_invalid"),
            "{offset}"
        );
    }
    for cursor in [
        raw_cursor(ID, REVISION, "0"),
        raw_cursor(OTHER, EMPTY_REVISION, "0"),
    ] {
        assert_eq!(
            reason_of(&fake, &cursor).as_deref(),
            Some("cursor_stale"),
            "{cursor}"
        );
    }
    assert_eq!(
        reason_of(&fake, &raw_cursor(ID, EMPTY_REVISION, "-1")).as_deref(),
        Some("cursor_invalid")
    );
}

#[test]
fn the_next_cursor_is_the_exact_bytes_of_python_json_dumps() {
    let fake = with_items(5);
    let (_, doc, _) = run(&fake, ID, &["--limit", "2"]);
    let cursor = doc["result"]["next_cursor"].as_str().unwrap();
    assert_eq!(
        cursor,
        "WyJhYWFhYWFhYS1hYWFhLTRhYWEtOGFhYS1hYWFhYWFhYWFhYWEiLCAiMDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWYiLCAyXQ=="
    );
    let text = String::from_utf8(URL_SAFE.decode(cursor).unwrap()).unwrap();
    assert_eq!(text, format!("[\"{ID}\", \"{REVISION}\", 2]"));
}

fn plan(directory: Option<(&str, u64, u64)>) -> PlanEvidence {
    PlanEvidence {
        tool_use_id: "toolu_01".into(),
        directory: directory.map(|(path, device, inode)| PlanDirectory {
            path: path.into(),
            device,
            inode,
        }),
    }
}

fn plan_page(first: Option<PlanEvidence>, second: Option<PlanEvidence>) -> (Value, String) {
    let mut fake = with_items(0);
    fake.history.manifest = Some(ManifestHead {
        header_valid: true,
        revision: REVISION.into(),
        count: 3,
    });
    fake.history.items = vec![item(0, first), item(1, second), item(2, None)];
    let (code, doc, stdout) = run(&fake, ID, &[]);
    assert_eq!(code, 0, "{stdout}");
    (doc, stdout)
}

#[test]
fn a_plan_reference_serves_its_tool_use_id_and_recorded_directory() {
    let (doc, stdout) = plan_page(
        Some(plan(Some((
            "/plans/p",
            u64::MAX,
            9_223_372_036_854_775_808,
        )))),
        Some(plan(None)),
    );
    let items = &doc["result"]["items"];
    assert_eq!(
        items[0]["provenance"],
        json!({
            "method": "observed_backend_plan",
            "plan_directory": "/plans/p",
            "plan_directory_identity": {"device": u64::MAX, "inode": 9_223_372_036_854_775_808u64},
            "task_id": ID,
            "tool_use_id": "toolu_01"
        })
    );
    assert_eq!(
        items[1]["provenance"],
        json!({"method": "observed_backend_plan", "task_id": ID, "tool_use_id": "toolu_01"})
    );
    assert_eq!(
        items[2]["provenance"],
        json!({"method": "verified_process_lineage", "task_id": ID})
    );
    // Key order as the sorted reference output: method, plan_directory, plan_directory_identity,
    // task_id, tool_use_id; the identity exact.
    for wanted in [
        format!(
            "\"provenance\": {{\"method\": \"observed_backend_plan\", \"plan_directory\": \"/plans/p\", \"plan_directory_identity\": {{\"device\": 18446744073709551615, \"inode\": 9223372036854775808}}, \"task_id\": \"{ID}\", \"tool_use_id\": \"toolu_01\"}}"
        ),
        format!(
            "\"provenance\": {{\"method\": \"observed_backend_plan\", \"task_id\": \"{ID}\", \"tool_use_id\": \"toolu_01\"}}"
        ),
    ] {
        assert!(stdout.contains(&wanted), "{wanted}\n{stdout}");
    }
}
