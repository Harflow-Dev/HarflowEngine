#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Encoded lengths of the artifact manifest. Every expected length below was computed with
//! Python's `len(json.dumps(value, ensure_ascii=False, allow_nan=False).encode('utf-8'))` on
//! the equivalent reference dict (product version `0.0.13+rust`), not with the code under test.
use hf_interfaces::artifact_codec::{
    MANIFEST_FORMAT, ManifestItem, envelope_len, item_len, item_value,
};
use serde_json::{Value, json};

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn simple(method: &'static str) -> ManifestItem<'static> {
    ManifestItem {
        task_id: A,
        artifact_id: "art-1",
        path: "/workspace/repo/out/a.txt",
        kind: "report",
        note: "n",
        registered_at: "2026-01-10T10:04:00+00:00",
        method,
        device: 1,
        inode: 2,
        size: 3,
        mtime_ns: 4,
        tool_use_id: None,
        plan_directory: None,
        workflow: None,
    }
}

/// Multibyte text, escapes, a control character, DEL, U+2028, the widest identity values,
/// the most negative mtime, plan evidence with a directory and a workflow binding.
fn complex(workflow: &Value) -> ManifestItem<'_> {
    ManifestItem {
        task_id: A,
        artifact_id: "art-\u{e9}",
        path: "/w/\u{e9}\u{6f22}\u{1f600}\u{2028}",
        kind: "other",
        note: "q\"b\\n\nt\tc\u{1}d\u{7f}\u{1f}",
        registered_at: "2026-01-10T10:04:00+00:00",
        method: "observed_backend_plan",
        device: u64::MAX,
        inode: 0,
        size: i64::MAX as u64,
        mtime_ns: i128::from(i64::MIN),
        tool_use_id: Some("toolu_01"),
        plan_directory: Some(("/plans/\u{e9}", u64::MAX, 9_223_372_036_854_775_808)),
        workflow: Some(workflow),
    }
}

fn binding() -> Value {
    json!({"run_id": "r-1", "node": "n\u{20ac}", "role": "node"})
}

#[test]
fn the_envelope_length_matches_the_reference() {
    assert_eq!(MANIFEST_FORMAT, "harflow.task.artifacts");
    assert_eq!(envelope_len("a"), 129);
    assert_eq!(envelope_len(A), 164);
    assert_eq!(envelope_len("t\u{e9}\u{6f22}\u{1f600}\"\\\u{1}"), 148);
}

#[test]
fn item_lengths_match_the_reference_for_each_provenance_shape() {
    assert_eq!(item_len(&simple("verified_process_lineage")), Some(373));
    assert_eq!(item_len(&simple("engine_output")), Some(362));
    let workflow = binding();
    assert_eq!(item_len(&complex(&workflow)), Some(654));
    let mut plan = simple("observed_backend_plan");
    plan.artifact_id = "p";
    plan.path = "/p";
    plan.note = "";
    plan.registered_at = "t";
    plan.device = 0;
    plan.inode = 0;
    plan.size = 0;
    plan.mtime_ns = i128::from(u64::MAX);
    plan.tool_use_id = Some("x");
    assert_eq!(item_len(&plan), Some(357));
}

#[test]
fn a_manifest_is_the_envelope_items_and_two_bytes_between_items() {
    // The whole three-item manifest, as the reference writes it, is 1557 bytes.
    let workflow = binding();
    let items = [
        simple("verified_process_lineage"),
        simple("engine_output"),
        complex(&workflow),
    ];
    let sum: usize = items.iter().map(|i| item_len(i).unwrap()).sum();
    assert_eq!(envelope_len(A) + sum + 2 * (items.len() - 1), 1557);
}

#[test]
fn a_plan_directory_is_written_only_with_a_tool_use_id() {
    let mut stray = simple("engine_output");
    stray.plan_directory = Some(("/plans", 1, 2));
    assert_eq!(item_len(&stray), item_len(&simple("engine_output")));
    let provenance = &item_value(&stray).unwrap()["provenance"];
    assert_eq!(
        provenance,
        &json!({"method": "engine_output", "task_id": A})
    );
}

#[test]
fn the_item_document_carries_every_reference_field() {
    let workflow = binding();
    let value = item_value(&complex(&workflow)).unwrap();
    assert_eq!(
        value,
        json!({
            "artifact_id": "art-\u{e9}",
            "task_id": A,
            "path": "/w/\u{e9}\u{6f22}\u{1f600}\u{2028}",
            "kind": "other",
            "note": "q\"b\\n\nt\tc\u{1}d\u{7f}\u{1f}",
            "registered_at": "2026-01-10T10:04:00+00:00",
            "provenance": {
                "method": "observed_backend_plan",
                "task_id": A,
                "tool_use_id": "toolu_01",
                "plan_directory": "/plans/\u{e9}",
                "plan_directory_identity": {"device": u64::MAX, "inode": 9_223_372_036_854_775_808_u64},
            },
            "registered_observation": {
                "device": u64::MAX,
                "inode": 0,
                "size": i64::MAX,
                "mtime_ns": i64::MIN,
            },
            "workflow": workflow,
        })
    );
}

#[test]
fn an_mtime_outside_both_64_bit_ranges_is_not_encodable() {
    let mut item = simple("engine_output");
    for (mtime, len) in [
        (i128::from(i64::MIN), Some(381)),
        (i128::from(u64::MAX), Some(381)),
        (-1, Some(363)),
    ] {
        item.mtime_ns = mtime;
        assert_eq!(item_len(&item), len, "{mtime}");
    }
    for mtime in [i128::from(i64::MIN) - 1, i128::from(u64::MAX) + 1] {
        item.mtime_ns = mtime;
        assert_eq!(item_len(&item), None, "{mtime}");
        assert!(item_value(&item).is_none(), "{mtime}");
    }
}
