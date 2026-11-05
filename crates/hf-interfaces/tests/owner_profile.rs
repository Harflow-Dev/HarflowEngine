#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_interfaces::owner_profile::classify;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/owner-runtime-v1")
}

#[test]
fn every_pinned_candidate_has_its_exact_normative_classification() {
    let root = fixture_root();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let entries = manifest["files"].as_array().unwrap();
    assert_eq!(entries.len(), 64);

    let mut seen = 0;
    for entry in entries {
        let name = entry["filename"].as_str().unwrap();
        let bytes = fs::read(root.join(name)).unwrap();
        assert_eq!(
            bytes.len(),
            entry["bytes"].as_u64().unwrap() as usize,
            "{name}"
        );
        let actual = classify(&bytes);
        assert_eq!(actual, entry["expected"], "fixture {name}");
        seen += 1;
    }
    assert_eq!(seen, 64);
}

#[test]
fn valid_storage_ack_ignores_one_sided_unknown_writer_metadata() {
    let root = fixture_root();
    let mut document: Value =
        serde_json::from_slice(&fs::read(root.join("storage-acknowledged.json")).unwrap()).unwrap();
    document["data"]["pending"]["writer"]["optional_note"] = json!("unknown on pending side");
    let actual = classify(serde_json::to_string(&document).unwrap().as_bytes());
    assert_eq!(
        actual,
        json!({
            "parse": "valid",
            "decision": "advance",
            "reason": "acknowledged"
        })
    );
}

#[test]
fn observation_keeps_agent_result_separate_from_workflow_and_resource_release() {
    let root = fixture_root();
    let mut document: Value = serde_json::from_slice(
        &fs::read(root.join("observation-incidental-pending.json")).unwrap(),
    )
    .unwrap();
    let actual = classify(serde_json::to_string(&document).unwrap().as_bytes());
    assert_eq!(actual["result"], "succeeded");
    assert_eq!(actual["workflow"], "succeeded");
    assert_eq!(actual["release"], "blocked");
    document["data"]["requirement"] = json!("unknown");
    let unknown_required = classify(serde_json::to_string(&document).unwrap().as_bytes());
    assert_eq!(unknown_required["workflow"], "blocked");
}

#[test]
fn late_ack_and_prior_poison_each_retain_mutation_admission() {
    let root = fixture_root();
    let late = fs::read(root.join("storage-late.json")).unwrap();
    assert_eq!(
        classify(&late),
        json!({
            "parse": "valid",
            "decision": "retain",
            "reason": "late_ack"
        })
    );

    let poisoned = fs::read(root.join("storage-poisoned.json")).unwrap();
    assert_eq!(
        classify(&poisoned),
        json!({
            "parse": "valid",
            "decision": "retain",
            "reason": "poisoned"
        })
    );

    let mut document: Value =
        serde_json::from_slice(&fs::read(root.join("storage-acknowledged.json")).unwrap()).unwrap();
    document["data"]["poisoned"] = json!(true);
    let actual = classify(serde_json::to_string(&document).unwrap().as_bytes());
    assert_eq!(
        actual,
        json!({
            "parse": "valid",
            "decision": "retain",
            "reason": "poisoned"
        })
    );
}

#[test]
fn stock_permission_unknown_refuses_and_unknown_fields_never_opt_into_tool_mode() {
    let root = fixture_root();
    let unknown_permission = fs::read(root.join("permission-unknown.json")).unwrap();
    assert_eq!(
        classify(&unknown_permission),
        json!({
            "parse": "valid",
            "decision": "refuse",
            "reason": "unqualified"
        })
    );

    let mut policy: Value =
        serde_json::from_slice(&fs::read(root.join("policy-default.json")).unwrap()).unwrap();
    let expected = classify(&serde_json::to_vec(&policy).unwrap());
    policy["unexpected_top_level"] = json!({ "permission_mode": "tool_level" });
    policy["binding"]["owner"]["unknown"] = json!("ignored context");
    policy["data"]["tool_level_opt_in"] = json!(false);
    let actual = classify(&serde_json::to_vec(&policy).unwrap());
    assert_eq!(actual, expected);
    assert_eq!(actual["permission_mode"], "os_enforced");
    assert_eq!(actual["tool_level_opt_in"], false);
}

#[test]
fn lexical_negative_zero_stays_an_integer_while_negative_zero_float_refuses() {
    let root = fixture_root();
    let source = fs::read_to_string(root.join("policy-default.json")).unwrap();
    let expected = classify(source.as_bytes());
    let integer_zero = source.replace("\"device\": 0", "\"device\": -0");
    assert_eq!(classify(integer_zero.as_bytes()), expected);
    let float_zero = source.replace("\"device\": 0", "\"device\": -0.0");
    assert_eq!(
        classify(float_zero.as_bytes()),
        json!({ "parse": "invalid", "reason": "binding" })
    );
}
