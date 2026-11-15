//! Graph rules over the normalized nodes and connections: loops, parallel boundaries,
//! legacy joins and optional nodes. The algorithms live in the domain.
use hf_domain::workflow::graph::parallel::{self, Group};
use hf_domain::workflow::graph::{BitSet, Graph, GraphEdge, GraphNode, dominance, optional};
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_graph::GraphRule;
use hf_domain::workflow::vocab::NodeType;

use super::connections::Edge;
use super::field::{Check, identifier};
use hf_domain::workflow::refusal::Refusal;

/// The checked nodes the graph rules start from.
pub struct Nodes {
    pub ids: Vec<String>,
    pub kinds: Vec<NodeType>,
    pub start: usize,
}

fn build(nodes: &[Json], info: &Nodes, edges: &[Edge]) -> Graph {
    let graph_nodes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| GraphNode {
            id: info.ids[index].clone(),
            kind: info.kinds[index],
            group: None,
            optional: node
                .as_object()
                .and_then(|map| map.get("optional"))
                .and_then(Json::as_bool)
                == Some(true),
        })
        .collect();
    let graph_edges = edges
        .iter()
        .map(|edge| GraphEdge {
            id: edge.id.clone(),
            source: edge.source,
            target: edge.target,
            backward: false,
        })
        .collect();
    Graph::new(graph_nodes, graph_edges)
}

fn node_at(nodes: &mut [Json], index: usize) -> Option<&mut JsonObject> {
    nodes.get_mut(index).and_then(Json::as_object_mut)
}

/// `_validate_parallel_boundaries`.
fn explicit_boundaries(
    nodes: &mut [Json],
    graph: &mut Graph,
    postdominators: &[BitSet],
    info: &Nodes,
) -> Check<()> {
    let mut groups: Vec<Group> = Vec::new();
    for index in 0..nodes.len() {
        let Some(node) = node_at(nodes, index) else {
            continue;
        };
        node.remove("join_id");
        let kind = info.kinds[index];
        if kind == NodeType::Join {
            return Err(refuse_graph(GraphRule::ParallelUseBoundaries));
        }
        if !matches!(kind, NodeType::ParallelStart | NodeType::ParallelEnd) {
            continue;
        }
        let name = identifier(node.get("parallel_group_id"))?;
        let at = match groups.iter().position(|group| group.id == name) {
            Some(found) => found,
            None => {
                groups.push(Group {
                    id: name.clone(),
                    start: None,
                    end: None,
                });
                groups.len() - 1
            }
        };
        let slot = if kind == NodeType::ParallelStart {
            &mut groups[at].start
        } else {
            &mut groups[at].end
        };
        if slot.is_some() {
            return Err(refuse_graph(GraphRule::ParallelDuplicateBoundary)
                .with_text(kind.as_str())
                .with_text(&name));
        }
        *slot = Some(index);
        graph.nodes[index].group = Some(at);
    }
    for (start, end) in parallel::check_groups(graph, &groups, postdominators)? {
        if let Some(node) = node_at(nodes, start) {
            node.insert("join_id", Json::text(&info.ids[end]));
        }
    }
    Ok(())
}

fn refuse_graph(rule: GraphRule) -> Refusal {
    Refusal::new(rule)
}

/// Legacy routing: every split gets the `join_id` of its nearest common convergence.
fn legacy_joins(
    nodes: &mut [Json],
    graph: &Graph,
    postdominators: &[BitSet],
    info: &Nodes,
) -> Check<()> {
    for index in 0..nodes.len() {
        let forward = graph.forward_edges(index);
        let Some(node) = node_at(nodes, index) else {
            continue;
        };
        if forward.len() <= 1 {
            node.remove("join_id");
            continue;
        }
        let id = &info.ids[index];
        let targets: Vec<usize> = forward
            .iter()
            .map(|edge| graph.edges[*edge].target)
            .collect();
        let closest = graph
            .closest_convergence(postdominators, &targets)
            .ok_or_else(|| refuse_graph(GraphRule::LegacyNoConvergence).with_text(id))?;
        let legacy = node
            .get("join_id")
            .and_then(Json::as_str)
            .and_then(|name| info.ids.iter().position(|other| other == name));
        if legacy.is_some_and(|found| info.kinds[found] == NodeType::Join && found != closest) {
            return Err(refuse_graph(GraphRule::LegacyCrossBranch).with_text(id));
        }
        node.insert("join_id", Json::text(&info.ids[closest]));
    }
    let inferred: Vec<&str> = nodes
        .iter()
        .filter_map(|node| node.as_object()?.get("join_id")?.as_str())
        .collect();
    for (index, id) in info.ids.iter().enumerate() {
        if info.kinds[index] == NodeType::Join && !inferred.contains(&id.as_str()) {
            return Err(refuse_graph(GraphRule::JoinOutsideSplit).with_text(id));
        }
    }
    Ok(())
}

/// Everything after the per-node and per-connection checks. Writes each connection's
/// inferred `backward` flag and every `join_id` into the JSON.
pub fn validate(
    explicit: bool,
    nodes: &mut [Json],
    connections: &mut [Json],
    info: &Nodes,
    edges: &[Edge],
) -> Check<()> {
    let mut graph = build(nodes, info, edges);
    dominance::analyze(&mut graph, info.start)?;
    for (item, edge) in connections.iter_mut().zip(&graph.edges) {
        if let Some(map) = item.as_object_mut() {
            map.insert("backward", Json::Bool(edge.backward));
        }
    }
    let postdominators = graph.postdominators();
    if explicit {
        explicit_boundaries(nodes, &mut graph, &postdominators, info)?;
    } else {
        legacy_joins(nodes, &graph, &postdominators, info)?;
    }
    optional::validate(&graph, &postdominators, explicit)
}
