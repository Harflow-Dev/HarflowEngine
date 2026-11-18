//! Memoization must preserve path-sensitive depth refusals.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_snapshot_support;

use hf_application::workflow::snapshot_scopes::{ResourceScopeFailure, validate_tree_scopes};
use workflow_snapshot_support::{entry, resource_definition, tree, workflow_node};

#[test]
fn shallow_completed_child_does_not_hide_deeper_fifth_level_path() {
    for shallow_first in [true, false] {
        let shared = workflow_node("shared-call", "shared");
        let parent = workflow_node("parent-call", "parent");
        let root_nodes = if shallow_first {
            vec![shared, parent]
        } else {
            vec![parent, shared]
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
                resource_definition("parent", None, vec![workflow_node("next", "middle")]),
                "parent",
            ),
            entry(
                "middle",
                "middle",
                resource_definition("middle", None, vec![workflow_node("next", "shared")]),
                "middle",
            ),
            entry(
                "shared",
                "shared",
                resource_definition("shared", None, vec![workflow_node("next", "leaf")]),
                "shared",
            ),
            entry(
                "leaf",
                "leaf",
                resource_definition("leaf", None, vec![]),
                "leaf",
            ),
        ];
        assert_eq!(
            validate_tree_scopes(&tree("root", entries, vec![])),
            Err(ResourceScopeFailure::CycleOrDepth),
        );
    }
}

#[test]
fn maximum_four_level_path_remains_valid() {
    let entries = vec![
        entry(
            "root",
            "root",
            resource_definition("root", None, vec![workflow_node("next", "parent")]),
            "root",
        ),
        entry(
            "parent",
            "parent",
            resource_definition("parent", None, vec![workflow_node("next", "middle")]),
            "parent",
        ),
        entry(
            "middle",
            "middle",
            resource_definition("middle", None, vec![workflow_node("next", "shared")]),
            "middle",
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
