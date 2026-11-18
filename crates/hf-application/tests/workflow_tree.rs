//! Pinned tree projections use parent insertion order and strict access semantics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::tree::{
    AccessSummary, DependencyTree, TreeEdge, TreeEntry, access_summary, referencing_definitions,
};
use hf_domain::workflow::vocab::Freedom;

fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect(),
    )
}
fn entry(name: &str) -> (String, TreeEntry) {
    (
        name.into(),
        TreeEntry {
            name: name.into(),
            revision: Json::int(1),
            definition_sha256: name.into(),
            definition: JsonObject::new(),
        },
    )
}
fn access(depth: usize) -> (String, AccessSummary) {
    (
        "x".into(),
        AccessSummary {
            max_freedom: Freedom::ReadOnly,
            network: false,
            backends: vec![],
            depth,
        },
    )
}

#[test]
fn subtree_keeps_parent_order_for_workflows_access_and_edges() {
    let tree = DependencyTree {
        root_workflow_id: "r".into(),
        workflows: ["r", "z", "x", "y", "q"].into_iter().map(entry).collect(),
        edges: vec![
            TreeEdge {
                from: "r".into(),
                node_id: "rz".into(),
                to: "z".into(),
                orchestrator_mode: "child".into(),
            },
            TreeEdge {
                from: "r".into(),
                node_id: "rx".into(),
                to: "x".into(),
                orchestrator_mode: "child".into(),
            },
            TreeEdge {
                from: "z".into(),
                node_id: "zq".into(),
                to: "q".into(),
                orchestrator_mode: "child".into(),
            },
            TreeEdge {
                from: "x".into(),
                node_id: "xy".into(),
                to: "y".into(),
                orchestrator_mode: "child".into(),
            },
        ],
        access: ["r", "z", "x", "y", "q"]
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id.into(), access(i + 1).1))
            .collect(),
    };
    let subtree = tree.subtree("r").unwrap();
    assert_eq!(
        subtree
            .workflows
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["r", "z", "x", "y", "q"]
    );
    assert_eq!(
        subtree
            .access
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["r", "z", "x", "y", "q"]
    );
    assert_eq!(
        subtree
            .edges
            .iter()
            .map(|edge| edge.node_id.as_str())
            .collect::<Vec<_>>(),
        ["rz", "rx", "zq", "xy"]
    );
}

#[test]
fn subtree_from_child_excludes_unreachable_parent_and_sibling() {
    let tree = DependencyTree {
        root_workflow_id: "r".into(),
        workflows: ["r", "x", "y"].into_iter().map(entry).collect(),
        edges: vec![
            TreeEdge {
                from: "r".into(),
                node_id: "rx".into(),
                to: "x".into(),
                orchestrator_mode: "child".into(),
            },
            TreeEdge {
                from: "r".into(),
                node_id: "ry".into(),
                to: "y".into(),
                orchestrator_mode: "current".into(),
            },
        ],
        access: ["r", "x", "y"]
            .into_iter()
            .map(|id| (id.into(), access(1).1))
            .collect(),
    };
    let sub = tree.subtree("x").unwrap();
    assert_eq!(sub.root_workflow_id, "x");
    assert_eq!(
        sub.workflows
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        ["x"]
    );
    assert!(sub.edges.is_empty());
    assert!(tree.subtree("absent").is_none());
}

#[test]
fn access_summary_requires_boolean_true_for_network() {
    for value in [
        Json::int(1),
        Json::int(0),
        Json::Float(1.0),
        Json::text("true"),
    ] {
        let agent = object(&[
            ("id", Json::text("a")),
            ("type", Json::text("agent")),
            ("network", value),
        ]);
        let definition = object(&[("nodes", Json::Array(vec![Json::Object(agent)]))]);
        assert!(!access_summary(&definition, 1).network);
    }
    let agent = object(&[
        ("id", Json::text("a")),
        ("type", Json::text("agent")),
        ("network", Json::Bool(true)),
    ]);
    let definition = object(&[("nodes", Json::Array(vec![Json::Object(agent)]))]);
    assert!(access_summary(&definition, 1).network);
}

#[test]
fn referencing_definitions_is_sorted_and_deduplicated() {
    let reference = || Json::Object(object(&[("workflow_id", Json::text("target"))]));
    let workflow = |id: &str| {
        Json::Object(object(&[
            ("id", Json::text(id)),
            ("type", Json::text("workflow")),
            ("workflow_ref", reference()),
        ]))
    };
    let make = |name: &str, nodes: Vec<Json>| {
        object(&[("name", Json::text(name)), ("nodes", Json::Array(nodes))])
    };
    let definitions = vec![
        make("zeta", vec![workflow("one"), workflow("two")]),
        make("alpha", vec![workflow("three")]),
    ];
    assert_eq!(
        referencing_definitions(&definitions, "target"),
        ["alpha", "zeta"]
    );
}
