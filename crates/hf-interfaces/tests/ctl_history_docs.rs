#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The `task-prompt` and `task-artifacts` documents and the artifact summary over a scripted store.
mod ctl_reads_support;
use base64::{Engine, engine::general_purpose::URL_SAFE};
use ctl_reads_support::*;
use hf_domain::ctl::{
    caller::PidObservation,
    history::{ArtifactItem, Availability, ManifestHead, Observation, PromptBlob, StoredPrompt},
};
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::{Value, json};

const REVISION: &str = "0123456789abcdef0123456789abcdef";

fn settled() -> Fake {
    fake("completed", Some(0), PidObservation::Absent)
}

fn artifact(number: u64, observation: Observation) -> ArtifactItem {
    ArtifactItem {
        artifact_id: format!("x{number}"),
        path: format!("/repo/out/f{number}.txt"),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-01T00:00:00+00:00".into(),
        method: "verified_process_lineage".into(),
        plan: None,
        registered: observation,
    }
}

fn plain(number: u64) -> ArtifactItem {
    artifact(
        number,
        Observation {
            device: 1,
            inode: number,
            size: 3,
            mtime_ns: 4,
        },
    )
}

fn with_items(count: u64) -> Fake {
    let mut fake = settled();
    fake.history.manifest = Some(ManifestHead {
        header_valid: true,
        revision: REVISION.into(),
        count: count as usize,
    });
    fake.history.items = (0..count).map(plain).collect();
    fake
}

fn artifacts(fake: &Fake, extra: &[&str]) -> (i32, Value, String) {
    let mut list = vec!["task-artifacts", ID, "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), fake);
    (
        out.code,
        serde_json::from_str(&out.stdout).unwrap_or(Value::Null),
        out.stderr,
    )
}

#[test]
fn observations_are_printed_as_exact_integers() {
    let mut fake = settled();
    let huge = artifact(
        0,
        Observation {
            device: u64::MAX,
            inode: u64::MAX - 1,
            size: u64::MAX >> 1,
            mtime_ns: i64::MIN,
        },
    );
    fake.history.manifest = Some(ManifestHead {
        header_valid: true,
        revision: REVISION.into(),
        count: 1,
    });
    fake.history.items = vec![huge];
    let out = dispatch(&args(&["task-artifacts", ID, "--json"]), &fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    for wanted in [
        "\"device\": 18446744073709551615",
        "\"inode\": 18446744073709551614",
        "\"mtime_ns\": -9223372036854775808",
    ] {
        assert!(out.stdout.contains(wanted), "{wanted}: {}", out.stdout);
    }
}

#[test]
fn an_entry_has_the_documented_keys_and_the_observed_state() {
    let mut fake = with_items(2);
    fake.targets
        .states
        .insert("x1".into(), Availability::Deleted);
    let (code, doc, _) = artifacts(&fake, &[]);
    assert_eq!(code, 0);
    let result = &doc["result"];
    let mut keys: Vec<&String> = result.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "count",
            "has_more",
            "items",
            "next_cursor",
            "state",
            "task_id"
        ]
    );
    let first = &result["items"][0];
    let mut entry: Vec<&String> = first.as_object().unwrap().keys().collect();
    entry.sort();
    assert_eq!(
        entry,
        [
            "artifact_id",
            "availability",
            "kind",
            "note",
            "path",
            "provenance",
            "registered_at",
            "registered_observation",
            "task_id"
        ]
    );
    assert_eq!(
        first["provenance"],
        json!({"method": "verified_process_lineage", "task_id": ID})
    );
    assert_eq!(first["availability"]["state"], "available");
    assert_eq!(result["items"][1]["availability"]["state"], "deleted");
    assert!(
        first["availability"]["observed_at"]
            .as_str()
            .unwrap()
            .ends_with("+00:00")
    );
}

#[test]
fn the_time_of_observation_is_null_without_a_clock() {
    let mut fake = with_items(1);
    fake.clock = false;
    let (_, doc, _) = artifacts(&fake, &[]);
    assert_eq!(
        doc["result"]["items"][0]["availability"]["observed_at"],
        Value::Null
    );
}

#[test]
fn the_next_cursor_is_padded_urlsafe_base64_of_task_revision_offset() {
    let fake = with_items(5);
    let (_, doc, _) = artifacts(&fake, &["--limit", "2"]);
    let cursor = doc["result"]["next_cursor"].as_str().unwrap();
    assert_eq!(
        URL_SAFE.decode(cursor).unwrap(),
        format!("[\"{ID}\", \"{REVISION}\", 2]").into_bytes()
    );
    let (_, next, _) = artifacts(&fake, &["--limit", "2", "--cursor", cursor]);
    assert_eq!(next["result"]["items"][0]["artifact_id"], "x2");
    assert_eq!(next["result"]["has_more"], true);
}

#[test]
fn only_a_task_revision_integer_triple_is_a_cursor() {
    let fake = with_items(5);
    let stale = "Invalid or stale artifact cursor";
    let make = |value: Value| URL_SAFE.encode(value.to_string());
    for (label, cursor) in [
        ("float", make(json!([ID, REVISION, 1.0]))),
        (
            "beyond i64",
            make(json!([ID, REVISION, 9_223_372_036_854_775_808u64])),
        ),
        ("text offset", make(json!([ID, REVISION, "1"]))),
        ("null offset", make(json!([ID, REVISION, null]))),
        ("numeric task", make(json!([1, REVISION, 1]))),
        ("short", make(json!([ID, REVISION]))),
        ("long", make(json!([ID, REVISION, 1, 2]))),
        ("object", make(json!({"a": 1}))),
        ("empty", String::new()),
        ("unicode", "\u{e9}".repeat(8)),
        ("too long", make(json!([ID, REVISION, 1, "p".repeat(600)]))),
    ] {
        let (code, doc, _) = artifacts(&fake, &["--cursor", &cursor]);
        assert_eq!(code, 1, "{label}");
        assert_eq!(doc["error"]["code"], "artifact_error", "{label}");
        assert_eq!(doc["error"]["message"], stale, "{label}");
    }
}

#[test]
fn a_padding_stripped_cursor_is_stale_as_the_references_decoder_refuses_it() {
    let fake = with_items(5);
    let text = json!([ID, REVISION, 3]).to_string();
    let padded = URL_SAFE.encode(&text);
    assert!(
        padded.ends_with('='),
        "the test needs a cursor that has padding"
    );
    let (code, doc, _) = artifacts(&fake, &["--cursor", &padded]);
    assert_eq!(
        (code, doc["result"]["items"][0]["artifact_id"].as_str()),
        (0, Some("x3"))
    );
    let (code, doc, _) = artifacts(&fake, &["--cursor", padded.trim_end_matches('=')]);
    assert_eq!(code, 1);
    assert_eq!(doc["error"]["code"], "artifact_error");
    assert_eq!(doc["error"]["message"], "Invalid or stale artifact cursor");
}

#[test]
fn artifacts_need_no_json_flag_and_are_unavailable_where_targets_cannot_be_observed() {
    let fake = with_items(1);
    let out = dispatch(&args(&["task-artifacts", ID]), &fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let mut blind = with_items(1);
    blind.artifact_support = false;
    let out = dispatch(&args(&["task-artifacts", ID, "--json"]), &blind);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("invalid choice"), "{}", out.stderr);
}

#[test]
fn the_root_marker_makes_every_artifact_read_an_artifact_error() {
    let mut fake = with_items(1);
    fake.root_marker = true;
    let (code, doc, stderr) = artifacts(&fake, &[]);
    assert_eq!(code, 1);
    assert_eq!(doc["error"]["code"], "artifact_error");
    assert!(
        stderr.starts_with("harflow-ctl: error: Task artifacts are refused"),
        "{stderr}"
    );
}

#[test]
fn artifact_failures_are_all_artifact_errors_with_their_own_wording() {
    let fake = with_items(1);
    for (extra, message) in [
        (
            vec!["--limit", "0"],
            "Artifact limit must be between 1 and 50",
        ),
        (
            vec!["--limit", "51"],
            "Artifact limit must be between 1 and 50",
        ),
    ] {
        let (_, doc, _) = artifacts(&fake, &extra);
        assert_eq!(
            (
                doc["error"]["code"].as_str(),
                doc["error"]["message"].as_str()
            ),
            (Some("artifact_error"), Some(message))
        );
    }
    let out = dispatch(&args(&["task-artifacts", "bad/id", "--json"]), &fake);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(doc["error"]["message"], "not a valid task id: 'bad/id'");
}

#[test]
fn an_unreadable_manifest_is_an_unavailable_page_with_no_items() {
    let mut fake = with_items(3);
    fake.history.manifest = Some(ManifestHead {
        header_valid: false,
        revision: REVISION.into(),
        count: 3,
    });
    let (_, doc, _) = artifacts(&fake, &[]);
    assert_eq!(doc["result"]["state"], "unavailable");
    assert_eq!(
        (
            doc["result"]["items"].clone(),
            doc["result"]["count"].clone()
        ),
        (json!([]), json!(0))
    );
}

fn prompt_of(stored: StoredPrompt, extra: &[&str]) -> Value {
    let mut fake = settled();
    fake.history.prompt = Some(stored);
    let mut list = vec!["task-prompt", ID, "--json"];
    list.extend_from_slice(extra);
    let out = dispatch(&args(&list), &fake);
    assert_eq!(out.code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout).unwrap()["result"].clone()
}

#[test]
fn the_prompt_document_carries_the_title_and_the_exact_view() {
    let stored = StoredPrompt {
        title: Some("Named".into()),
        inline: Some("h\u{e9}llo".into()),
        preview: None,
        bound: false,
        blob: PromptBlob::Absent,
    };
    let result = prompt_of(stored, &[]);
    assert_eq!(result["task_id"], ID);
    assert_eq!(result["title"], "Named");
    assert_eq!(
        result["prompt"],
        json!({"state": "available", "text": "h\u{e9}llo", "returned_bytes": 6, "total_bytes": 6,
               "truncated": false, "complete": true, "integrity": "not_applicable"})
    );
}

#[test]
fn the_bound_cuts_on_a_character_edge_and_defaults_to_sixty_five_thousand_five_hundred_and_thirty_six()
 {
    let text = "\u{e9}".repeat(40_000);
    let stored = StoredPrompt {
        title: None,
        inline: Some(text),
        preview: None,
        bound: false,
        blob: PromptBlob::Absent,
    };
    let result = prompt_of(stored.clone(), &[]);
    assert_eq!(result["prompt"]["returned_bytes"], 65_536);
    assert_eq!(result["prompt"]["truncated"], true);
    let result = prompt_of(stored, &["--max-bytes", "3"]);
    assert_eq!(
        (
            result["prompt"]["returned_bytes"].clone(),
            result["prompt"]["text"].clone()
        ),
        (json!(2), json!("\u{e9}"))
    );
}

#[test]
fn a_summary_in_the_status_document_follows_the_manifest() {
    let fake = with_items(3);
    let out = dispatch(&args(&["status", ID, "--json"]), &fake);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        doc["task"]["artifact_summary"],
        json!({"count": 3, "state": "available", "detail_ref": {"tool": "get_task_artifacts", "task_id": ID}})
    );
}

#[test]
fn a_command_failure_of_the_target_port_is_an_artifact_error_with_its_message() {
    let mut fake = with_items(2);
    fake.targets.failure = Some("'getpwuid(): uid not found: 4242'".into());
    let (code, doc, stderr) = artifacts(&fake, &[]);
    assert_eq!(code, 1);
    assert_eq!(doc["error"]["code"], "artifact_error");
    assert_eq!(doc["error"]["message"], "'getpwuid(): uid not found: 4242'");
    assert_eq!(
        stderr,
        "harflow-ctl: error: 'getpwuid(): uid not found: 4242'\n"
    );
    let out = dispatch(&args(&["status", ID, "--json"]), &fake);
    assert_eq!(
        out.code, 0,
        "the summary of a status never asks the port: {}",
        out.stderr
    );
}
