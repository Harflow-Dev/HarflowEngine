//! Independent tests of the pure R10-A launch and snapshot rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
mod workflow_snapshot_support;

use hf_application::workflow::ports::DefinitionCodec;
use hf_application::workflow::snapshot::{SnapshotFailure, launch_definition, prepare_snapshot};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::vocab::MAX_JSON_DEPTH;
use workflow_parity_support::RealCodec;
use workflow_snapshot_support::{definition, entry, object, orchestration, tree, validator};

#[test]
fn backend_change_drops_exactly_six_settings_and_retains_other_keys() {
    let mut source = definition("launch");
    let settings = [
        ("model", Json::text("old-model")),
        ("reasoning_effort", Json::text("high")),
        ("max_turns", Json::int(9)),
        ("background_idle_seconds", Json::int(30)),
        ("caller_approvals", Json::Bool(true)),
        ("caller_write_approvals", Json::Bool(true)),
        ("conversation_profile", Json::text("native")),
    ];
    orchestration(&mut source, &settings);
    source
        .get_mut("orchestrator")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("backend", Json::text("claude"));
    let original = source.clone();
    let overrides = object(&[("backend", Json::text("codex"))]);
    let launched = launch_definition(&validator(), &source, Some(&overrides)).unwrap();
    let actual = launched.get("orchestrator").unwrap().as_object().unwrap();
    for key in [
        "model",
        "reasoning_effort",
        "max_turns",
        "background_idle_seconds",
    ] {
        assert!(!actual.contains_key(key), "{key} survived a backend change");
    }
    // Validation inserts the destination backend's default false value after the six
    // source-specific values are discarded.
    assert_eq!(actual.get("caller_approvals"), Some(&Json::Bool(false)));
    assert!(!actual.contains_key("caller_write_approvals"));
    assert_eq!(
        actual.get("conversation_profile"),
        Some(&Json::text("native"))
    );
    assert_eq!(actual.get("backend"), Some(&Json::text("codex")));
    assert_eq!(source, original, "launch must not mutate its input");
}

#[test]
fn same_backend_and_empty_overrides_retain_settings() {
    let mut source = definition("launch");
    orchestration(
        &mut source,
        &[("model", Json::text("m")), ("max_turns", Json::int(11))],
    );
    source
        .get_mut("orchestrator")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("backend", Json::text("claude"));
    let same = object(&[
        ("backend", Json::text("claude")),
        ("model", Json::text("override")),
    ]);
    let launched = launch_definition(&validator(), &source, Some(&same)).unwrap();
    let agent = launched.get("orchestrator").unwrap().as_object().unwrap();
    assert_eq!(agent.get("max_turns"), Some(&Json::int(11)));
    assert_eq!(agent.get("model"), Some(&Json::text("override")));
    let empty = JsonObject::new();
    let launched = launch_definition(&validator(), &source, Some(&empty)).unwrap();
    let agent = launched.get("orchestrator").unwrap().as_object().unwrap();
    assert_eq!(agent.get("model"), Some(&Json::text("m")));
    assert_eq!(agent.get("max_turns"), Some(&Json::int(11)));
}

#[test]
fn snapshot_refuses_source_name_before_pin_validation() {
    let root = definition("actual");
    let root_before = root.clone();
    let bad_pin = entry("different", "different", JsonObject::new(), "not-a-hash");
    let pins = tree("different", vec![bad_pin], vec![]);
    let pins_before = pins.clone();
    assert_eq!(
        prepare_snapshot(&validator(), &RealCodec, "expected", &root, None, &pins),
        Err(SnapshotFailure::SourceNameMismatch),
    );
    assert_eq!(root, root_before);
    assert_eq!(pins, pins_before);
}

#[test]
fn snapshot_revalidates_pins_in_insertion_order_before_hashing() {
    let root = definition("root");
    let first = entry("first", "bad-first", JsonObject::new(), "bad");
    let second = entry(
        "second",
        "bad-second",
        object(&[("name", Json::text("second"))]),
        "bad",
    );
    let pins = tree("absent-root", vec![first, second], vec![]);
    let pins_before = pins.clone();
    let outcome = prepare_snapshot(&validator(), &RealCodec, "root", &root, None, &pins);
    let expected_first = validator()
        .validate(&Json::Object(JsonObject::new()))
        .unwrap_err();
    let actual = match outcome {
        Err(SnapshotFailure::Validation(actual)) => actual,
        other => panic!("unexpected result: {other:?}"),
    };
    assert_eq!(
        hf_interfaces::workflow::messages::failure_message(&actual),
        "Workflow names must contain 1–100 letters, digits, spaces, dots, dashes or underscores, start with a letter or digit, and have no trailing spaces",
    );
    assert_eq!(
        hf_interfaces::workflow::messages::failure_message(&actual),
        hf_interfaces::workflow::messages::failure_message(&expected_first)
    );
    assert_eq!(pins, pins_before, "failed snapshot must not mutate any pin");
    let valid_wrong_hash = entry(
        "first",
        "valid-first",
        definition("valid-first"),
        "false-hash",
    );
    let later_invalid = entry("second", "invalid-second", JsonObject::new(), "unused");
    let pins = tree("absent-root", vec![valid_wrong_hash, later_invalid], vec![]);
    assert_eq!(
        prepare_snapshot(&validator(), &RealCodec, "root", &root, None, &pins),
        Err(SnapshotFailure::PinHashMismatch)
    );
}

#[test]
fn snapshot_requires_present_root_with_matching_name_and_hash() {
    let root = launch_definition(&validator(), &definition("root"), None).unwrap();
    let codec = RealCodec;
    let hash = hf_application::workflow::identity::definition_sha256(&codec, &root);
    let missing = tree(
        "absent",
        vec![entry("root-id", "root", root.clone(), &hash)],
        vec![],
    );
    assert_eq!(
        prepare_snapshot(&validator(), &codec, "root", &root, None, &missing),
        Err(SnapshotFailure::RootMismatch)
    );
    let wrong_name = tree(
        "root-id",
        vec![entry("root-id", "other", root.clone(), &hash)],
        vec![],
    );
    assert_eq!(
        prepare_snapshot(&validator(), &codec, "root", &root, None, &wrong_name),
        Err(SnapshotFailure::RootMismatch)
    );
    let wrong_hash = tree(
        "root-id",
        vec![entry("root-id", "root", root.clone(), "00")],
        vec![],
    );
    assert_eq!(
        prepare_snapshot(&validator(), &codec, "root", &root, None, &wrong_hash),
        Err(SnapshotFailure::PinHashMismatch)
    );
    let mut other = definition("root");
    other.insert("description", Json::text("different valid snapshot"));
    let other = launch_definition(&validator(), &other, None).unwrap();
    let other_hash = hf_application::workflow::identity::definition_sha256(&codec, &other);
    let wrong_root_hash = tree(
        "root-id",
        vec![entry("root-id", "root", other, &other_hash)],
        vec![],
    );
    assert_eq!(
        prepare_snapshot(&validator(), &codec, "root", &root, None, &wrong_root_hash),
        Err(SnapshotFailure::RootMismatch)
    );
}

#[test]
fn snapshot_forces_and_independently_rehashes_every_pin_without_mutating_input() {
    let codec = RealCodec;
    let root = launch_definition(&validator(), &definition("root"), None).unwrap();
    let mut child = definition("child");
    child.insert("context_delivery", Json::text("legacy"));
    child.insert("custom_metadata", Json::text("preserve original pin"));
    let root_hash = hf_application::workflow::identity::definition_sha256(&codec, &root);
    let child_hash = hf_application::workflow::identity::definition_sha256(&codec, &child);
    let mut expected_child = child.clone();
    expected_child.insert("context_delivery", Json::text("optimized_v1"));
    let source_before = root.clone();
    let original = tree(
        "root-id",
        vec![
            entry("root-id", "root", root.clone(), &root_hash),
            entry("child-id", "child", child.clone(), &child_hash),
        ],
        vec![],
    );
    let tree_before = original.clone();
    let snapshot = prepare_snapshot(&validator(), &codec, "root", &root, None, &original).unwrap();
    assert_eq!(
        root, source_before,
        "snapshot must not mutate the source definition"
    );
    assert_eq!(
        original, tree_before,
        "snapshot must not mutate the pinned input tree"
    );
    assert_eq!(
        snapshot.dependency_tree.workflows[1].1.definition,
        expected_child
    );
    assert!(
        !snapshot.dependency_tree.workflows[1]
            .1
            .definition
            .contains_key("schema_version")
    );
    assert!(
        !snapshot.dependency_tree.workflows[1]
            .1
            .definition
            .contains_key("orchestrator")
    );
    assert_eq!(
        snapshot.dependency_tree.workflows[1].1.definition_sha256,
        hf_application::workflow::identity::definition_sha256(
            &codec,
            &snapshot.dependency_tree.workflows[1].1.definition,
        )
    );
    for (_, pinned) in &snapshot.dependency_tree.workflows {
        assert_eq!(
            pinned.definition.get("context_delivery"),
            Some(&Json::text("optimized_v1"))
        );
        let expected =
            hf_application::workflow::identity::definition_sha256(&codec, &pinned.definition);
        assert_eq!(pinned.definition_sha256, expected);
    }
}

struct CountingCodec(std::cell::Cell<usize>);

impl DefinitionCodec for CountingCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String {
        RealCodec.sha256_hex(bytes)
    }
    fn definition_sha256(&self, definition: &JsonObject) -> String {
        self.0.set(self.0.get() + 1);
        RealCodec.definition_sha256(definition)
    }
    fn ascii_dumps_len(&self, value: &Json) -> usize {
        RealCodec.ascii_dumps_len(value)
    }
}

fn too_deep_value() -> Json {
    let mut value = Json::Null;
    for _ in 0..=MAX_JSON_DEPTH {
        value = Json::Array(vec![value]);
    }
    value
}

#[test]
fn depth_guards_cover_source_overrides_pins_and_revision_metadata() {
    let codec = CountingCodec(std::cell::Cell::new(0));
    let mut source = definition("root");
    source.insert("metadata", too_deep_value());
    assert!(matches!(
        launch_definition(&validator(), &source, None),
        Err(SnapshotFailure::Validation(_))
    ));
    let overrides = object(&[("metadata", too_deep_value())]);
    assert!(matches!(
        launch_definition(&validator(), &definition("root"), Some(&overrides)),
        Err(SnapshotFailure::Validation(_))
    ));
    let deep_pin = entry(
        "root-id",
        "root",
        {
            let mut value = definition("root");
            value.insert("metadata", too_deep_value());
            value
        },
        "bad",
    );
    let pins = tree("root-id", vec![deep_pin], vec![]);
    assert!(matches!(
        prepare_snapshot(
            &validator(),
            &codec,
            "root",
            &definition("root"),
            None,
            &pins
        ),
        Err(SnapshotFailure::Validation(_))
    ));
    assert_eq!(
        codec.0.get(),
        0,
        "oversized input must be rejected before digesting"
    );

    let launched = launch_definition(&validator(), &definition("root"), None).unwrap();
    let digest = RealCodec.definition_sha256(&launched);
    let mut revision_pin = entry("root-id", "root", launched, &digest);
    revision_pin.1.revision = too_deep_value();
    let pins = tree("root-id", vec![revision_pin], vec![]);
    assert!(matches!(
        prepare_snapshot(
            &validator(),
            &codec,
            "root",
            &definition("root"),
            None,
            &pins
        ),
        Err(SnapshotFailure::Validation(_))
    ));
}
