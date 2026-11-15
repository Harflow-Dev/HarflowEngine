//! Graph rules that no validated definition reaches, exercised on the graph directly:
//! overlapping parallel branches, End inside a group, a retry into a closed branch and
//! the selection-join refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use hf_domain::workflow::graph::parallel::{Group, check_groups};
use hf_domain::workflow::graph::{BitSet, Graph, GraphEdge, GraphNode, dominance, optional};
use hf_domain::workflow::json::Json;
use hf_domain::workflow::refusal::{Refusal, Rule};
use hf_domain::workflow::rule_graph::GraphRule;
use hf_domain::workflow::vocab::NodeType;
use hf_interfaces::workflow::messages::message;

/// `(id, kind, group, optional)` and `(source, target, backward)` by node id.
type NodeSpec = (&'static str, NodeType, Option<usize>, bool);

fn graph(nodes: &[NodeSpec], edges: &[(&str, &str, bool)]) -> Graph {
    let index = |id: &str| nodes.iter().position(|node| node.0 == id).unwrap();
    let nodes = nodes
        .iter()
        .map(|(id, kind, group, optional)| GraphNode {
            id: id.to_string(),
            kind: *kind,
            group: *group,
            optional: *optional,
        })
        .collect();
    let edges = edges
        .iter()
        .map(|(source, target, backward)| GraphEdge {
            id: format!("{source}-{target}"),
            source: index(source),
            target: index(target),
            backward: *backward,
        })
        .collect();
    Graph::new(nodes, edges)
}

fn group(id: &str, start: usize, end: usize) -> Group {
    Group {
        id: id.to_string(),
        start: Some(start),
        end: Some(end),
    }
}

fn check(graph: &Graph, groups: &[Group]) -> Result<Vec<(usize, usize)>, Refusal> {
    let postdominators = graph.postdominators();
    check_groups(graph, groups, &postdominators)
}

fn detail(refusal: &Refusal) -> Vec<&str> {
    refusal.detail.iter().filter_map(Json::as_str).collect()
}

const PS: NodeType = NodeType::ParallelStart;
const PE: NodeType = NodeType::ParallelEnd;
const AGENT: NodeType = NodeType::Agent;

#[test]
fn parallel_overlap_refused() {
    let nodes = [
        ("s", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("b", AGENT, None, false),
        ("q", PE, Some(0), false),
    ];
    let graph = graph(
        &nodes,
        &[
            ("s", "a", false),
            ("s", "b", false),
            ("a", "b", false),
            ("b", "q", false),
        ],
    );
    let refusal = check(&graph, &[group("g", 0, 3)]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::ParallelOverlap));
    assert_eq!(detail(&refusal), ["g"]);
    assert_eq!(
        message(&refusal),
        "Parallel group g has crossing or overlapping sibling branches"
    );
}

#[test]
fn identical_branch_targets_overlap() {
    let nodes = [
        ("s", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("q", PE, Some(0), false),
    ];
    let edges = [("s", "a", false), ("s", "a", false), ("a", "q", false)];
    let refusal = check(&graph(&nodes, &edges), &[group("g", 0, 2)]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::ParallelOverlap));
}

#[test]
fn end_inside_a_group_refused() {
    let nodes = [
        ("s", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("z", NodeType::End, None, false),
        ("b", AGENT, None, false),
        ("q", PE, Some(0), false),
    ];
    let edges = [
        ("s", "a", false),
        ("s", "b", false),
        ("a", "z", false),
        ("z", "q", false),
        ("b", "q", false),
    ];
    let refusal = check(&graph(&nodes, &edges), &[group("g", 0, 4)]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::ParallelReachesEnd));
    assert_eq!(
        message(&refusal),
        "Parallel group g reaches workflow End before its matching end"
    );
}

#[test]
fn retry_entering_a_closed_branch_refused() {
    // Only a group whose start and end are the same node reaches this rule: an edge from a
    // distinct end into a branch is an external entry first.
    let nodes = [
        ("p", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("b", AGENT, None, false),
    ];
    let graph = graph(&nodes, &[("p", "a", false), ("p", "b", false)]);
    let refusal = check(&graph, &[group("g", 0, 0)]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::ParallelRetryEntry));
    assert_eq!(detail(&refusal), ["p-a"]);
    assert_eq!(
        message(&refusal),
        "Retry cannot enter a closed parallel branch: p-a"
    );
}

#[test]
fn a_distinct_end_retry_is_an_external_entry() {
    let nodes = [
        ("s", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("b", AGENT, None, false),
        ("q", PE, Some(0), false),
    ];
    let edges = [
        ("s", "a", false),
        ("s", "b", false),
        ("a", "q", false),
        ("b", "q", false),
        ("q", "a", true),
    ];
    let refusal = check(&graph(&nodes, &edges), &[group("g", 0, 3)]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::ParallelExternalEntry));
    assert_eq!(detail(&refusal), ["g", "q-a"]);
}

#[test]
fn a_well_formed_group_returns_its_bounds() {
    let nodes = [
        ("s", PS, Some(0), false),
        ("a", AGENT, None, false),
        ("b", AGENT, None, false),
        ("q", PE, Some(0), false),
    ];
    let edges = [
        ("s", "a", false),
        ("s", "b", false),
        ("a", "q", false),
        ("b", "q", false),
    ];
    assert_eq!(
        check(&graph(&nodes, &edges), &[group("g", 0, 3)]).unwrap(),
        [(0, 3)]
    );
}

#[test]
fn incomplete_group_refused_by_name() {
    let nodes = [("s", PS, Some(0), false), ("a", AGENT, None, false)];
    let graph = graph(&nodes, &[("s", "a", false)]);
    let incomplete = Group {
        id: "g".to_string(),
        start: Some(0),
        end: None,
    };
    let refusal = check(&graph, &[incomplete]).unwrap_err();
    assert_eq!(
        refusal.rule,
        Rule::Graph(GraphRule::ParallelGroupIncomplete)
    );
}

#[test]
fn selection_join_message_from_optional() {
    let nodes = [
        ("s", AGENT, None, false),
        ("a", AGENT, None, true),
        ("b", AGENT, None, false),
    ];
    let graph = graph(&nodes, &[("s", "a", false), ("s", "b", false)]);
    let postdominators: Vec<BitSet> = graph.postdominators();
    let refusal = optional::selection_join(&graph, &postdominators, &[1, 2]).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::SelectionJoinAmbiguous));
    assert!(refusal.detail.is_empty());
    assert_eq!(
        message(&refusal),
        "Selected parallel paths must converge at one common node"
    );
    let from_validate = optional::validate(&graph, &postdominators, false).unwrap_err();
    assert_eq!(
        from_validate.rule,
        Rule::Graph(GraphRule::SelectionJoinAmbiguous)
    );
}

#[test]
fn selection_join_names_the_convergence() {
    let nodes = [
        ("s", AGENT, None, false),
        ("a", AGENT, None, true),
        ("b", AGENT, None, false),
        ("j", AGENT, None, false),
    ];
    let graph = graph(
        &nodes,
        &[
            ("s", "a", false),
            ("s", "b", false),
            ("a", "j", false),
            ("b", "j", false),
        ],
    );
    let postdominators = graph.postdominators();
    assert_eq!(
        optional::selection_join(&graph, &postdominators, &[1, 2]).unwrap(),
        3
    );
    assert!(optional::validate(&graph, &postdominators, false).is_ok());
    assert_eq!(graph.closest_convergence(&postdominators, &[]), None);
}

#[test]
fn explicit_routing_skips_splits_that_are_not_parallel_starts() {
    let nodes = [
        ("s", AGENT, None, false),
        ("a", AGENT, None, true),
        ("b", AGENT, None, false),
    ];
    let graph = graph(&nodes, &[("s", "a", false), ("s", "b", false)]);
    let postdominators = graph.postdominators();
    let refusal = optional::validate(&graph, &postdominators, true).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::OptionalUnsafe));
    assert_eq!(refusal.detail, [Json::texts(&["a".to_string()])]);
}

#[test]
fn dominance_marks_backward_edges() {
    let nodes = [
        ("s", NodeType::Start, None, false),
        ("a", AGENT, None, false),
        ("b", AGENT, None, false),
        ("e", NodeType::End, None, false),
    ];
    let mut graph = graph(
        &nodes,
        &[
            ("s", "a", false),
            ("a", "b", false),
            ("b", "a", false),
            ("b", "e", false),
        ],
    );
    dominance::analyze(&mut graph, 0).unwrap();
    let flags: Vec<bool> = graph.edges.iter().map(|edge| edge.backward).collect();
    assert_eq!(flags, [false, false, true, false]);
}

#[test]
fn analysis_refusals_are_deterministic() {
    let nodes = [
        ("s", NodeType::Start, None, false),
        ("a", AGENT, None, false),
        ("e", NodeType::End, None, false),
        ("x", AGENT, None, false),
        ("w", AGENT, None, false),
    ];
    let mut unreachable = graph(
        &nodes,
        &[
            ("s", "a", false),
            ("a", "e", false),
            ("x", "w", false),
            ("w", "e", false),
        ],
    );
    let refusal = dominance::analyze(&mut unreachable, 0).unwrap_err();
    assert_eq!(refusal.rule, Rule::Graph(GraphRule::UnreachableFromStart));
}
