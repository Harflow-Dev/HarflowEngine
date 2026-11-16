//! Dependency resolution: depth, occurrence order, refusals and bounded diagnostics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_support;

use hf_application::workflow::diagnostics::missing_references;
use hf_application::workflow::identity::workflow_identity;
use hf_application::workflow::ports::DefinitionCodec;
use hf_application::workflow::references::{
    ReferenceFailure, indexed_definitions, resolve_dependencies,
};
use hf_application::workflow::validate::{Validator, WorkflowFailure};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::DependencyRule;
use hf_domain::workflow::vocab::MAX_JSON_DEPTH;
use std::cell::Cell;
use std::collections::HashMap;
use workflow_support::{FakeCodec, MapResolver, TableCatalog};

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect(),
    )
}
fn node(id: &str, kind: &str) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("type", Json::text(kind)),
    ]))
}
fn edge(id: &str, from: &str, to: &str) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("source", Json::text(from)),
        ("target", Json::text(to)),
    ]))
}
fn definition(name: &str, id: &str, refs: &[(&str, &str)]) -> JsonObject {
    let mut nodes = vec![node("start", "start")];
    for (node_id, target) in refs {
        let reference = Json::Object(object(&[("workflow_id", Json::text(target))]));
        nodes.push(Json::Object(object(&[
            ("id", Json::text(node_id)),
            ("type", Json::text("workflow")),
            ("workflow_ref", reference),
        ])));
    }
    nodes.push(node("end", "end"));
    let mut edges = Vec::new();
    for pair in nodes.windows(2) {
        let from = pair[0]
            .as_object()
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap();
        let to = pair[1]
            .as_object()
            .unwrap()
            .get("id")
            .unwrap()
            .as_str()
            .unwrap();
        edges.push(edge(&format!("e{}", edges.len()), from, to));
    }
    object(&[
        ("name", Json::text(name)),
        ("workflow_id", Json::text(id)),
        ("routing_mode", Json::text("explicit")),
        ("nodes", Json::Array(nodes)),
        ("connections", Json::Array(edges)),
    ])
}
fn resolve(
    root: &JsonObject,
    definitions: &[JsonObject],
) -> Result<hf_domain::workflow::tree::DependencyTree, ReferenceFailure> {
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    // Stored catalog entries are normalized definitions. Matching Python's store snapshot
    // keeps their hashes equal to the resolver's normalized root pin.
    let stored = definitions
        .iter()
        .map(|definition| {
            validator
                .validate(&Json::Object(definition.clone()))
                .expect("catalog fixture is a valid stored definition")
        })
        .collect::<Vec<_>>();
    resolve_dependencies(
        &validator,
        &FakeCodec,
        &Json::Object(root.clone()),
        &stored,
        None,
    )
}
fn dependency_error(error: ReferenceFailure) -> hf_domain::workflow::refusal::DependencyRefusal {
    match error {
        ReferenceFailure::Dependency(value) => value,
        other => panic!("{other:?}"),
    }
}

#[test]
fn shallow_then_deep_diamond_keeps_first_pin_order_and_max_depth() {
    let a = definition("A", "a", &[("to_d", "d"), ("to_b", "b")]);
    let b = definition("B", "b", &[("to_c", "c")]);
    let c = definition("C", "c", &[("to_d", "d")]);
    let d = definition("D", "d", &[]);
    let tree = resolve(&a, &[a.clone(), b, c, d]).unwrap();
    assert_eq!(
        tree.workflows
            .iter()
            .map(|(_, e)| e.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "D", "B", "C"]
    );
    assert_eq!(tree.access_of("d").unwrap().depth, 4);
    assert_eq!(
        tree.edges
            .iter()
            .map(|e| e.node_id.as_str())
            .collect::<Vec<_>>(),
        ["to_d", "to_b", "to_c", "to_d"]
    );
}

#[test]
fn deep_then_shallow_diamond_updates_max_depth_without_repinning() {
    let a = definition("A", "a", &[("to_b", "b"), ("to_d", "d")]);
    let b = definition("B", "b", &[("to_c", "c")]);
    let c = definition("C", "c", &[("to_d", "d")]);
    let d = definition("D", "d", &[]);
    let tree = resolve(&a, &[a.clone(), b, c, d]).unwrap();
    assert_eq!(tree.access_of("d").unwrap().depth, 4);
    assert_eq!(
        tree.workflows
            .iter()
            .map(|(_, e)| e.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "C", "D"]
    );
    assert_eq!(
        tree.edges
            .iter()
            .map(|e| e.node_id.as_str())
            .collect::<Vec<_>>(),
        ["to_b", "to_c", "to_d", "to_d"]
    );
}

#[test]
fn cycle_and_unknown_reference_preserve_first_error_with_occurrence_diagnostics() {
    let root = definition("Root", "root", &[("missing_a", "absent"), ("loop", "root")]);
    let failure = dependency_error(resolve(&root, std::slice::from_ref(&root)).unwrap_err());
    assert_eq!(failure.refusal.rule, DependencyRule::Unknown.into());
    assert_eq!(failure.missing_references.len(), 1);
    assert_eq!(failure.missing_references[0].segment.node_id, "missing_a");
    assert_eq!(failure.missing_references[0].target_workflow_id, "absent");
}

#[test]
fn missing_occurrence_diagnostic_preserves_unicode_and_truncates_display_by_characters() {
    let mut root = definition("Root", "root", &[("missing_a", "absent")]);
    let nodes = root.get_mut("nodes").and_then(Json::as_array_mut).unwrap();
    nodes[1]
        .as_object_mut()
        .unwrap()
        .insert("title", Json::text(&"😀é".repeat(150)));
    let failure = dependency_error(resolve(&root, std::slice::from_ref(&root)).unwrap_err());
    let report = &failure.missing_references[0];
    assert_eq!(report.segment.node_id, "missing_a");
    assert_eq!(report.segment.node_title.chars().count(), 200);
    assert!(report.segment.node_title.starts_with("😀é"));
    assert!(!failure.truncated);
}

#[test]
fn missing_occurrence_reports_stop_at_the_fixed_limit() {
    let refs: Vec<(String, String)> = (0..257)
        .map(|i| (format!("m{i}"), format!("absent{i}")))
        .collect();
    let borrowed: Vec<(&str, &str)> = refs
        .iter()
        .map(|(node, target)| (node.as_str(), target.as_str()))
        .collect();
    let root = definition("Root", "root", &borrowed);
    let failure = dependency_error(resolve(&root, std::slice::from_ref(&root)).unwrap_err());
    assert_eq!(failure.missing_references.len(), 256);
    assert!(failure.truncated);
    assert_eq!(failure.missing_references[0].segment.node_id, "m0");
    assert_eq!(failure.missing_references[255].segment.node_id, "m255");
}

#[test]
fn missing_occurrence_reports_stop_at_the_encoded_byte_limit() {
    let target = "x".repeat(1024);
    let refs: Vec<(String, String)> = (0..180)
        .map(|i| (format!("m{i}"), target.clone()))
        .collect();
    let borrowed: Vec<(&str, &str)> = refs
        .iter()
        .map(|(node, target)| (node.as_str(), target.as_str()))
        .collect();
    let root = definition("Root", "root", &borrowed);
    let failure = dependency_error(resolve(&root, std::slice::from_ref(&root)).unwrap_err());
    assert!(failure.missing_references.len() < 256);
    assert!(failure.missing_references.len() > 50);
    assert!(failure.truncated);
    assert!(
        failure
            .missing_references
            .iter()
            .all(|report| report.target_workflow_id.len() == 1024)
    );
}

fn normalized_index(
    validator: &Validator<'_>,
    definitions: &[JsonObject],
) -> HashMap<String, JsonObject> {
    definitions
        .iter()
        .map(|definition| {
            let normalized = validator
                .validate(&Json::Object(definition.clone()))
                .expect("diagnostic fixture validates");
            (workflow_identity(&FakeCodec, &normalized), normalized)
        })
        .collect()
}

fn fanout_refs(prefix: &str, count: usize, target: &str) -> Vec<(String, String)> {
    (0..count)
        .map(|index| (format!("{prefix}{index}"), target.to_string()))
        .collect()
}

fn definition_with_refs(name: &str, id: &str, refs: &[(String, String)]) -> JsonObject {
    let borrowed = refs
        .iter()
        .map(|(node, target)| (node.as_str(), target.as_str()))
        .collect::<Vec<_>>();
    definition(name, id, &borrowed)
}

#[test]
fn diagnostics_stop_at_ten_thousand_visits_before_report_limit() {
    let root = definition_with_refs("Root", "root", &fanout_refs("r", 10, "a"));
    let a = definition_with_refs("A", "a", &fanout_refs("a", 10, "b"));
    let b = definition_with_refs("B", "b", &fanout_refs("b", 10, "c"));
    let c = definition_with_refs("C", "c", &fanout_refs("c", 10, "d"));
    let d = definition("D", "d", &[]);
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    let root = validator.validate(&Json::Object(root)).unwrap();
    let index = normalized_index(&validator, &[a, b, c, d]);
    let (reports, truncated) = missing_references(&validator, &FakeCodec, &root, &index);
    assert!(reports.is_empty(), "depth-four branches point to valid D");
    assert!(
        truncated,
        "more than 10,000 repeated node visits must stop traversal"
    );
}

#[test]
fn diagnostics_suppress_overlong_target_ids_and_mark_truncated() {
    let target = "x".repeat(1025);
    let refs = [("missing", target.as_str())];
    let raw = definition("Root", "root", &refs);
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    let root = validator.validate(&Json::Object(raw)).unwrap();
    let (reports, truncated) = missing_references(&validator, &FakeCodec, &root, &HashMap::new());
    assert!(reports.is_empty());
    assert!(truncated);
}

#[test]
fn nesting_refusal_reports_bounded_path_at_fifth_level() {
    let defs = vec![
        definition("A", "a", &[("ab", "b")]),
        definition("B", "b", &[("bc", "c")]),
        definition("C", "c", &[("cd", "d")]),
        definition("D", "d", &[("de", "e")]),
        definition("E", "e", &[]),
    ];
    let failure = dependency_error(resolve(&defs[0], &defs).unwrap_err());
    assert_eq!(failure.refusal.rule, DependencyRule::Nesting.into());
    assert_eq!(
        failure.refusal.detail.last(),
        Some(&Json::text("A > B > C > D > e > E"))
    );
}

#[test]
fn duplicate_identity_with_different_hash_is_rejected() {
    let first = definition("First", "same", &[]);
    let second = definition("Second", "same", &[]);
    let error =
        dependency_error(indexed_definitions(&FakeCodec, &[first, second], None).unwrap_err());
    assert_eq!(error.refusal.rule, DependencyRule::Duplicate.into());
}

struct CountingCodec(Cell<usize>);
impl DefinitionCodec for CountingCodec {
    fn sha256_hex(&self, bytes: &[u8]) -> String {
        FakeCodec.sha256_hex(bytes)
    }
    fn definition_sha256(&self, definition: &JsonObject) -> String {
        self.0.set(self.0.get() + 1);
        FakeCodec.definition_sha256(definition)
    }
    fn ascii_dumps_len(&self, value: &Json) -> usize {
        FakeCodec.ascii_dumps_len(value)
    }
}

#[test]
fn overdeep_catalog_definition_is_refused_before_hashing_or_recursive_clone() {
    let mut nested = Json::Null;
    for _ in 0..=MAX_JSON_DEPTH {
        nested = Json::Array(vec![nested]);
    }
    let mut raw = definition("Too deep", "deep", &[]);
    raw.insert("extension", nested);
    let codec = CountingCodec(Cell::new(0));
    let result = indexed_definitions(&codec, &[raw], None);
    assert!(matches!(
        result,
        Err(ReferenceFailure::Validation(WorkflowFailure::Refused(_)))
    ));
    assert_eq!(
        codec.0.get(),
        0,
        "catalog hash must not visit the overdeep raw value"
    );
}
