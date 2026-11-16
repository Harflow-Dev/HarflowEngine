//! Explicit routing: parallel groups, their boundary rules, and optional nodes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_validate_support;
use workflow_validate_support::*;

use hf_domain::workflow::json::Json;

/// `(id, type, extra fields)` of a node.
type NodeSpec<'a> = (&'a str, &'a str, &'a str);

fn explicit(nodes: &[NodeSpec<'_>], edges: &[(&str, &str)]) -> String {
    let nodes: Vec<String> = nodes
        .iter()
        .map(|(id, kind, extra)| {
            let tail = if extra.is_empty() {
                String::new()
            } else {
                format!(",{extra}")
            };
            format!(r#"{{"id":"{id}","type":"{kind}"{tail}}}"#)
        })
        .collect();
    let links: Vec<String> = edges
        .iter()
        .map(|(source, target)| {
            format!(r#"{{"id":"{source}-{target}","source":"{source}","target":"{target}"}}"#)
        })
        .collect();
    format!(
        r#"{{"name":"flow","routing_mode":"explicit","nodes":[{}],"connections":[{}]}}"#,
        nodes.join(","),
        links.join(",")
    )
}

const G: &str = r#""parallel_group_id":"g""#;

/// Start, p(g), x, y, q(g), End with the given extra edges.
fn group(
    extra_nodes: &[NodeSpec<'_>],
    extra_edges: &[(&str, &str)],
    branch_edges: &[(&str, &str)],
) -> String {
    let mut nodes: Vec<NodeSpec<'_>> = vec![("s", "start", ""), ("p", "parallel_start", G)];
    nodes.extend_from_slice(extra_nodes);
    nodes.extend([("q", "parallel_end", G), ("e", "end", "")]);
    let mut edges = vec![("s", "p"), ("q", "e")];
    edges.extend_from_slice(branch_edges);
    edges.extend_from_slice(extra_edges);
    explicit(&nodes, &edges)
}

const XY: [NodeSpec<'static>; 2] = [("x", "agent", ""), ("y", "agent", "")];
const XY_EDGES: [(&str, &str); 4] = [("p", "x"), ("p", "y"), ("x", "q"), ("y", "q")];

#[test]
fn explicit_join_id_popped_then_appended() {
    let input = explicit(
        &[
            ("s", "start", ""),
            (
                "p",
                "parallel_start",
                r#""join_id":"stale","parallel_group_id":"g""#,
            ),
            ("x", "agent", r#""join_id":"stale""#),
            ("y", "agent", ""),
            ("q", "parallel_end", G),
            ("e", "end", ""),
        ],
        &[
            ("s", "p"),
            ("p", "x"),
            ("p", "y"),
            ("x", "q"),
            ("y", "q"),
            ("q", "e"),
        ],
    );
    let definition = accepted(&input);
    let start = at(&definition, "nodes.1").unwrap();
    assert_eq!(keys(start).last(), Some(&"join_id"));
    assert_eq!(at(&definition, "nodes.1.join_id"), Some(&Json::text("q")));
    assert_eq!(at(&definition, "nodes.2.join_id"), None);
    let places = keys(start).iter().filter(|key| **key == "join_id").count();
    assert_eq!(places, 1);
}

#[test]
fn a_valid_group_is_accepted() {
    let definition = accepted(&group(&XY, &[], &XY_EDGES));
    assert_eq!(at(&definition, "nodes.1.join_id"), Some(&Json::text("q")));
}

#[test]
fn nested_groups_are_accepted() {
    let nodes: [NodeSpec<'_>; 8] = [
        ("s", "start", ""),
        ("po", "parallel_start", r#""parallel_group_id":"outer""#),
        ("a", "agent", ""),
        ("ph", "parallel_start", r#""parallel_group_id":"inner""#),
        ("c", "agent", ""),
        ("d", "agent", ""),
        ("qh", "parallel_end", r#""parallel_group_id":"inner""#),
        ("b", "agent", ""),
    ];
    let mut all = nodes.to_vec();
    all.extend([
        ("qo", "parallel_end", r#""parallel_group_id":"outer""#),
        ("e", "end", ""),
    ]);
    let edges = [
        ("s", "po"),
        ("po", "a"),
        ("po", "b"),
        ("a", "ph"),
        ("ph", "c"),
        ("ph", "d"),
        ("c", "qh"),
        ("d", "qh"),
        ("qh", "qo"),
        ("b", "qo"),
        ("qo", "e"),
    ];
    let definition = accepted(&explicit(&all, &edges));
    assert_eq!(at(&definition, "nodes.1.join_id"), Some(&Json::text("qo")));
    assert_eq!(at(&definition, "nodes.3.join_id"), Some(&Json::text("qh")));
}

#[test]
fn boundary_declaration_rules() {
    let join = explicit(
        &[("s", "start", ""), ("j", "join", ""), ("e", "end", "")],
        &[("s", "j"), ("j", "e")],
    );
    assert_eq!(
        refused(&join),
        "Use Parallel start and Parallel end instead of Join"
    );
    let twice = explicit(
        &[
            ("s", "start", ""),
            ("p", "parallel_start", G),
            ("p2", "parallel_start", G),
            ("e", "end", ""),
        ],
        &[("s", "p"), ("p", "p2"), ("p2", "e")],
    );
    assert_eq!(
        refused(&twice),
        "Duplicate parallel_start for parallel group g"
    );
    let no_end = explicit(
        &[
            ("s", "start", ""),
            ("p", "parallel_start", G),
            ("x", "agent", ""),
            ("e", "end", ""),
        ],
        &[("s", "p"), ("p", "x"), ("x", "e")],
    );
    assert_eq!(
        refused(&no_end),
        "Parallel group g requires matching start and end"
    );
    let unnamed = explicit(
        &[
            ("s", "start", ""),
            ("p", "parallel_start", ""),
            ("e", "end", ""),
        ],
        &[("s", "p"), ("p", "e")],
    );
    assert!(refused(&unnamed).starts_with("Identifiers must contain"));
}

#[test]
fn start_needs_two_forward_branches_and_no_retries() {
    let one = group(&[("x", "agent", "")], &[], &[("p", "x"), ("x", "q")]);
    let message = "Parallel start p requires at least two forward branches and no retry outputs";
    assert_eq!(refused(&one), message);
    let nodes = [
        ("s", "start", ""),
        ("a", "agent", ""),
        ("p", "parallel_start", G),
        ("x", "agent", ""),
        ("y", "agent", ""),
        ("q", "parallel_end", G),
        ("e", "end", ""),
    ];
    let edges = [
        ("s", "a"),
        ("a", "p"),
        ("p", "x"),
        ("p", "y"),
        ("p", "a"),
        ("x", "q"),
        ("y", "q"),
        ("q", "e"),
    ];
    assert_eq!(refused(&explicit(&nodes, &edges)), message);
}

#[test]
fn every_branch_must_reach_the_matching_end() {
    let escaping = group(&XY, &[("y", "e")], &[("p", "x"), ("p", "y"), ("x", "q")]);
    assert_eq!(
        refused(&escaping),
        "Every branch from Parallel start p must reach matching Parallel end q"
    );
}

#[test]
fn empty_branch_refused() {
    let empty = group(
        &[("y", "agent", "")],
        &[],
        &[("p", "q"), ("p", "y"), ("y", "q")],
    );
    assert_eq!(
        refused(&empty),
        "Parallel start p cannot have an empty branch"
    );
}

#[test]
fn external_entry_into_a_branch() {
    let nodes = [
        ("s", "start", ""),
        ("o", "agent", ""),
        ("p", "parallel_start", G),
        ("x", "agent", ""),
        ("y", "agent", ""),
        ("q", "parallel_end", G),
        ("e", "end", ""),
    ];
    let edges = [
        ("s", "o"),
        ("o", "p"),
        ("o", "x"),
        ("p", "x"),
        ("p", "y"),
        ("x", "q"),
        ("y", "q"),
        ("q", "e"),
    ];
    assert_eq!(
        refused(&explicit(&nodes, &edges)),
        "External entry into parallel branch g: o-x"
    );
}

#[test]
fn retry_leaving_a_branch_escapes() {
    let nodes = [
        ("s", "start", ""),
        ("a", "agent", ""),
        ("p", "parallel_start", G),
        ("x", "agent", ""),
        ("y", "agent", ""),
        ("q", "parallel_end", G),
        ("e", "end", ""),
    ];
    let edges = [
        ("s", "a"),
        ("a", "p"),
        ("p", "x"),
        ("p", "y"),
        ("x", "q"),
        ("x", "a"),
        ("y", "q"),
        ("q", "e"),
    ];
    assert_eq!(
        refused(&explicit(&nodes, &edges)),
        "Connection escapes parallel branch g: x-a"
    );
}

#[test]
fn external_entry_into_the_end() {
    let nodes = [
        ("s", "start", ""),
        ("o", "agent", ""),
        ("p", "parallel_start", G),
        ("x", "agent", ""),
        ("y", "agent", ""),
        ("q", "parallel_end", G),
        ("e", "end", ""),
    ];
    let edges = [
        ("s", "o"),
        ("o", "p"),
        ("o", "q"),
        ("p", "x"),
        ("p", "y"),
        ("x", "q"),
        ("y", "q"),
        ("q", "e"),
    ];
    assert_eq!(
        refused(&explicit(&nodes, &edges)),
        "External entry into Parallel end q"
    );
}

#[test]
fn improper_nesting_names_the_outer_group() {
    let nodes = [
        ("s", "start", ""),
        ("po", "parallel_start", r#""parallel_group_id":"outer""#),
        ("a", "agent", ""),
        ("ph", "parallel_start", r#""parallel_group_id":"inner""#),
        ("c", "agent", ""),
        ("d", "agent", ""),
        ("b", "agent", ""),
        ("qo", "parallel_end", r#""parallel_group_id":"outer""#),
        ("qh", "parallel_end", r#""parallel_group_id":"inner""#),
        ("e", "end", ""),
    ];
    let edges = [
        ("s", "po"),
        ("po", "a"),
        ("po", "b"),
        ("a", "ph"),
        ("ph", "c"),
        ("ph", "d"),
        ("c", "qo"),
        ("d", "qo"),
        ("b", "qo"),
        ("qo", "qh"),
        ("qh", "e"),
    ];
    assert_eq!(
        refused(&explicit(&nodes, &edges)),
        "Parallel group outer contains an improperly nested boundary"
    );
}

#[test]
fn legacy_routing_has_no_parallel_boundaries() {
    let legacy = group(&XY, &[], &XY_EDGES).replace(r#""routing_mode":"explicit","#, "");
    assert_eq!(
        refused(&legacy),
        "Parallel boundaries require explicit routing_mode"
    );
}
