//! Path-sensitive resource scope checks for R10-A snapshots.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_snapshot_support;

use hf_application::workflow::snapshot_scopes::{ResourceScopeFailure, validate_tree_scopes};
use hf_domain::workflow::json::Json;
use hf_domain::workflow::vocab::MAX_NESTING_DEPTH;
use workflow_snapshot_support::{entry, resource_definition, resource_node, tree, workflow_node};

#[test]
fn resource_scope_distinguishes_unconstrained_empty_and_subset_scopes() {
    let unconstrained = tree(
        "u",
        vec![entry(
            "u",
            "u",
            resource_definition("u", None, vec![resource_node("n", &["x"])]),
            "h",
        )],
        vec![],
    );
    assert_eq!(validate_tree_scopes(&unconstrained), Ok(()));
    let empty = tree(
        "e",
        vec![entry(
            "e",
            "e",
            resource_definition("e", Some(&[]), vec![resource_node("n", &["x"])]),
            "h",
        )],
        vec![],
    );
    assert!(matches!(
        validate_tree_scopes(&empty),
        Err(ResourceScopeFailure::NodeExceeds { .. })
    ));
    let subset = tree(
        "s",
        vec![entry(
            "s",
            "s",
            resource_definition("s", Some(&["x", "y"]), vec![resource_node("n", &["x"])]),
            "h",
        )],
        vec![],
    );
    assert_eq!(validate_tree_scopes(&subset), Ok(()));
}

#[test]
fn resource_scope_checks_child_and_on_demand_resources_against_parent() {
    let root = resource_definition("root", Some(&["a"]), vec![workflow_node("child-call", "c")]);
    let child = resource_definition("child", Some(&["b"]), vec![]);
    let snapshot_tree = tree(
        "r",
        vec![
            entry("r", "root", root, "r"),
            entry("c", "child", child, "c"),
        ],
        vec![],
    );
    assert!(
        matches!(validate_tree_scopes(&snapshot_tree), Err(ResourceScopeFailure::WorkflowExceeds { resources, .. }) if resources == vec!["b"])
    );
    let root = resource_definition("root", Some(&["a"]), vec![resource_node("regular", &["a"])]);
    let mut root = root;
    root.insert(
        "on_demand_nodes",
        Json::Array(vec![resource_node("lazy", &["b"])]),
    );
    let snapshot_tree = tree("r", vec![entry("r", "root", root, "r")], vec![]);
    assert!(
        matches!(validate_tree_scopes(&snapshot_tree), Err(ResourceScopeFailure::NodeExceeds { node_id, .. }) if node_id == "lazy")
    );
}

#[test]
fn shared_child_is_checked_against_each_inherited_scope_in_both_orders() {
    for parents in [[("p1", "a"), ("p2", "b")], [("p2", "b"), ("p1", "a")]] {
        let mut entries = Vec::new();
        for (id, allowed) in parents {
            entries.push(entry(
                id,
                id,
                resource_definition(id, Some(&[allowed]), vec![workflow_node("call", "shared")]),
                id,
            ));
        }
        entries.push(entry(
            "shared",
            "shared",
            resource_definition("shared", None, vec![resource_node("child-node", &["b"])]),
            "shared",
        ));
        let root = resource_definition(
            "root",
            None,
            vec![
                workflow_node("first", parents[0].0),
                workflow_node("second", parents[1].0),
            ],
        );
        entries.push(entry("root", "root", root, "root"));
        let tree = tree("root", entries, vec![]);
        assert!(
            matches!(validate_tree_scopes(&tree), Err(ResourceScopeFailure::NodeExceeds { workflow, .. }) if workflow == "shared")
        );
    }
}

#[test]
fn resource_scope_refuses_missing_children_cycles_and_excess_depth() {
    let missing = tree(
        "root",
        vec![entry(
            "root",
            "root",
            resource_definition("root", None, vec![workflow_node("m", "missing")]),
            "h",
        )],
        vec![],
    );
    assert_eq!(
        validate_tree_scopes(&missing),
        Err(ResourceScopeFailure::MissingDependency {
            workflow_id: "missing".to_string()
        })
    );
    let cycle = tree(
        "a",
        vec![
            entry(
                "a",
                "a",
                resource_definition("a", None, vec![workflow_node("to-b", "b")]),
                "a",
            ),
            entry(
                "b",
                "b",
                resource_definition("b", None, vec![workflow_node("to-a", "a")]),
                "b",
            ),
        ],
        vec![],
    );
    assert_eq!(
        validate_tree_scopes(&cycle),
        Err(ResourceScopeFailure::CycleOrDepth)
    );
    let mut entries = Vec::new();
    for depth in 0..=MAX_NESTING_DEPTH {
        entries.push(entry(
            &format!("n{depth}"),
            &format!("n{depth}"),
            resource_definition(
                &format!("n{depth}"),
                None,
                if depth < MAX_NESTING_DEPTH {
                    vec![workflow_node("next", &format!("n{}", depth + 1))]
                } else {
                    vec![]
                },
            ),
            &format!("n{depth}"),
        ));
    }
    let too_deep = tree("n0", entries, vec![]);
    assert_eq!(
        validate_tree_scopes(&too_deep),
        Err(ResourceScopeFailure::CycleOrDepth)
    );
}

#[test]
fn resource_scope_memo_handles_deeper_shallower_and_equal_context_revisits() {
    for deep_first in [true, false] {
        let root_nodes = if deep_first {
            vec![
                workflow_node("via-parent", "parent"),
                workflow_node("direct", "shared"),
            ]
        } else {
            vec![
                workflow_node("direct", "shared"),
                workflow_node("via-parent", "parent"),
            ]
        };
        let entries = vec![
            entry(
                "root",
                "root",
                resource_definition("root", None, root_nodes),
                "root",
            ),
            entry(
                "parent",
                "parent",
                resource_definition("parent", None, vec![workflow_node("shared", "shared")]),
                "parent",
            ),
            entry(
                "shared",
                "shared",
                resource_definition("shared", None, vec![]),
                "shared",
            ),
        ];
        assert_eq!(validate_tree_scopes(&tree("root", entries, vec![])), Ok(()));
    }
}
