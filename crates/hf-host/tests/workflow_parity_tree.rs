//! Pure dependency-tree and refusal parity with the pinned Python reference corpus.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_parity_support;
#[path = "../../hf-application/tests/workflow_support/mod.rs"]
mod workflow_support;

use hf_application::workflow::references::{ReferenceFailure, resolve_dependencies, subtree};
use hf_application::workflow::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::refusal::{DependencyRefusal, MissingReference, ReferenceSegment};
use hf_domain::workflow::tree::{AccessSummary, DependencyTree};
use hf_interfaces::workflow::messages::{failure_message, message};
use workflow_parity_support::{Case, RealCodec, load_cases, resolver_entries};
use workflow_support::{MapResolver, TableCatalog};

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
}

fn expected_tree(tree: &DependencyTree) -> JsonObject {
    let workflows = tree
        .workflows
        .iter()
        .map(|(id, entry)| {
            Json::Object(object(&[
                ("id", Json::text(id)),
                ("name", Json::text(&entry.name)),
                ("revision", entry.revision.clone()),
                ("definition_sha256", Json::text(&entry.definition_sha256)),
                ("definition", Json::Object(entry.definition.clone())),
            ]))
        })
        .collect();
    let edges = tree
        .edges
        .iter()
        .map(|edge| {
            Json::Object(object(&[
                ("from", Json::text(&edge.from)),
                ("node_id", Json::text(&edge.node_id)),
                ("to", Json::text(&edge.to)),
                ("orchestrator_mode", Json::text(&edge.orchestrator_mode)),
            ]))
        })
        .collect();
    let access = tree
        .access
        .iter()
        .map(|(id, summary)| access_row(id, summary))
        .collect();
    object(&[
        ("root_workflow_id", Json::text(&tree.root_workflow_id)),
        ("workflows", Json::Array(workflows)),
        ("edges", Json::Array(edges)),
        ("access", Json::Array(access)),
    ])
}

fn access_row(id: &str, summary: &AccessSummary) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("max_freedom", Json::text(summary.max_freedom.as_str())),
        ("network", Json::Bool(summary.network)),
        ("backends", Json::texts(&summary.backends)),
        ("depth", Json::int(summary.depth as i64)),
    ]))
}

fn segment_row(segment: &ReferenceSegment) -> Json {
    Json::Object(object(&[
        ("workflow_id", Json::text(&segment.workflow_id)),
        ("workflow_name", Json::text(&segment.workflow_name)),
        ("node_id", Json::text(&segment.node_id)),
        ("node_title", Json::text(&segment.node_title)),
    ]))
}

fn missing_row(missing: &MissingReference) -> Json {
    let mut row = match segment_row(&missing.segment) {
        Json::Object(row) => row,
        _ => unreachable!(),
    };
    row.insert(
        "target_workflow_id",
        Json::text(&missing.target_workflow_id),
    );
    row.insert(
        "dependency_path",
        Json::Array(missing.dependency_path.iter().map(segment_row).collect()),
    );
    Json::Object(row)
}

fn diagnostic_outcome(error: &DependencyRefusal) -> JsonObject {
    object(&[
        ("message", Json::text(&message(&error.refusal))),
        (
            "missing_references",
            Json::Array(error.missing_references.iter().map(missing_row).collect()),
        ),
        ("truncated", Json::Bool(error.truncated)),
    ])
}

fn check_subtree(
    expected: &JsonObject,
    case_id: &str,
    tree: &DependencyTree,
    target: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    match subtree(tree, target) {
        Ok(child) => {
            let workflow_ids: Vec<Json> = child
                .workflows
                .iter()
                .map(|(id, _)| Json::text(id))
                .collect();
            let access_ids: Vec<Json> = child.access.iter().map(|(id, _)| Json::text(id)).collect();
            let parent_workflow_order: Vec<Json> = tree
                .workflows
                .iter()
                .filter_map(|(id, _)| child.entry(id).map(|_| Json::text(id)))
                .collect();
            let parent_access_order: Vec<Json> = tree
                .access
                .iter()
                .filter_map(|(id, _)| child.access_of(id).map(|_| Json::text(id)))
                .collect();
            if expected.get("subtree_parent_order") != Some(&Json::Array(parent_workflow_order))
                || expected.get("subtree_parent_order") != Some(&Json::Array(workflow_ids.clone()))
            {
                failures.push(format!(
                    "{case_id}: subtree workflow order differs from parent"
                ));
            }
            if expected.get("subtree_access_parent_order")
                != Some(&Json::Array(parent_access_order))
                || expected.get("subtree_access_parent_order")
                    != Some(&Json::Array(access_ids.clone()))
            {
                failures.push(format!(
                    "{case_id}: subtree access order differs from parent"
                ));
            }
            if !same_strings(&workflow_ids, expected.get("subtree_workflow_ids")) {
                failures.push(format!("{case_id}: subtree workflow contents differ"));
            }
            if !same_strings(&access_ids, expected.get("subtree_access_ids")) {
                failures.push(format!("{case_id}: subtree access contents differ"));
            }
            if expected_tree(&child).get("edges") != expected.get("subtree_edges") {
                failures.push(format!("{case_id}: subtree edges differ"));
            }
        }
        Err(error) => failures.push(format!(
            "{case_id}: subtree refused: {}",
            message(&error.refusal)
        )),
    }
    failures
}

fn check_success(case: &Case, expected: &JsonObject, tree: &DependencyTree) -> Vec<String> {
    let mut failures = Vec::new();
    if expected.get("tree") != Some(&Json::Object(expected_tree(tree))) {
        failures.push(format!("{}: resolved tree differs", case.id));
    }
    if let Some(target) = expected.get("subtree_target").and_then(Json::as_str) {
        failures.extend(check_subtree(expected, &case.id, tree, target));
    }
    failures
}

fn check_case(validator: &Validator<'_>, case: &Case) -> Vec<String> {
    let expected = case.expected();
    let Json::Object(root) = case.get("root") else {
        return vec![format!("{}: no root", case.id)];
    };
    let Json::Array(catalog) = case.get("catalog") else {
        return vec![format!("{}: no catalog", case.id)];
    };
    let definitions = catalog
        .iter()
        .map(|item| item.as_object().unwrap().clone())
        .collect::<Vec<_>>();
    let outcome = resolve_dependencies(
        validator,
        &RealCodec,
        &Json::Object(root.clone()),
        &definitions,
        None,
    );
    match (case.get("ok"), outcome) {
        (Json::Bool(true), Ok(tree)) => check_success(case, expected, &tree),
        (Json::Bool(false), Err(ReferenceFailure::Dependency(error))) => {
            let got = diagnostic_outcome(&error);
            (expected != &got)
                .then(|| format!("{}: dependency error/diagnostics differ; expected {expected:?}, got {got:?}", case.id))
                .into_iter()
                .collect()
        }
        (_, Err(ReferenceFailure::Validation(error))) => vec![format!(
            "{}: validation failed before dependency resolution: {}",
            case.id,
            failure_message(&error)
        )],
        (_, Err(ReferenceFailure::Dependency(error))) => vec![format!(
            "{}: refused unexpectedly: {}",
            case.id,
            message(&error.refusal)
        )],
        (_, Ok(_)) => vec![format!("{}: resolved, expected refusal", case.id)],
    }
}

fn same_strings(got: &[Json], expected: Option<&Json>) -> bool {
    let Some(Json::Array(want)) = expected else {
        return false;
    };
    let mut left = got.iter().filter_map(Json::as_str).collect::<Vec<_>>();
    let mut right = want.iter().filter_map(Json::as_str).collect::<Vec<_>>();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

#[test]
fn dependency_resolution_and_subtree_match_python_corpus() {
    let resolver = MapResolver::new(resolver_entries());
    let validator = Validator::new(&TableCatalog, &resolver);
    let cases = load_cases("tree.json");
    assert_eq!(cases.len(), 7);
    let failures = cases
        .iter()
        .flat_map(|case| check_case(&validator, case))
        .collect::<Vec<_>>();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
