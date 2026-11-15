//! Optional nodes: a safe parallel branch with a required sibling and no required
//! successor before convergence.
use super::{BitSet, Graph};
use crate::workflow::json::Json;
use crate::workflow::refusal::Refusal;
use crate::workflow::rule_graph::GraphRule;
use crate::workflow::vocab::NodeType;

fn required_region(graph: &Graph, region: &BitSet) -> bool {
    let agents: Vec<usize> = region
        .members()
        .into_iter()
        .filter(|node| graph.nodes[*node].kind.is_executable())
        .collect();
    !agents.is_empty() && agents.iter().all(|node| !graph.nodes[*node].optional)
}

fn optional_suffix(graph: &Graph, node: usize, join: usize) -> bool {
    let region = graph.forward_reachable(node, Some(join));
    region.contains(node)
        && region.members().into_iter().all(|member| {
            let item = &graph.nodes[member];
            !item.kind.is_executable() || item.optional
        })
}

/// `_selection_join`: where the forward paths from `targets` converge.
pub fn selection_join(
    graph: &Graph,
    postdominators: &[BitSet],
    targets: &[usize],
) -> Result<usize, Refusal> {
    graph
        .closest_convergence(postdominators, targets)
        .ok_or_else(|| Refusal::new(GraphRule::SelectionJoinAmbiguous))
}

/// `validate_optional_nodes`. In explicit routing only parallel starts split.
pub fn validate(graph: &Graph, postdominators: &[BitSet], explicit: bool) -> Result<(), Refusal> {
    let optional: Vec<usize> = (0..graph.nodes.len())
        .filter(|index| graph.nodes[*index].optional && graph.nodes[*index].kind.is_executable())
        .collect();
    if optional.is_empty() {
        return Ok(());
    }
    let mut valid = BitSet::empty(graph.nodes.len());
    for split in 0..graph.nodes.len() {
        if explicit && graph.nodes[split].kind != NodeType::ParallelStart {
            continue;
        }
        let edges = graph.forward_edges(split);
        for (index, edge) in edges.iter().enumerate() {
            for other in &edges[..index] {
                let targets = [graph.edges[*edge].target, graph.edges[*other].target];
                let join = selection_join(graph, postdominators, &targets)?;
                let left = graph.forward_reachable(targets[0], Some(join));
                let right = graph.forward_reachable(targets[1], Some(join));
                if left.intersects(&right) {
                    continue;
                }
                for (region, sibling) in [(&left, &right), (&right, &left)] {
                    if required_region(graph, sibling) {
                        for node in &optional {
                            if region.contains(*node) && optional_suffix(graph, *node, join) {
                                valid.insert(*node);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut invalid: Vec<String> = optional
        .iter()
        .filter(|node| !valid.contains(**node))
        .map(|node| graph.nodes[*node].id.clone())
        .collect();
    if invalid.is_empty() {
        return Ok(());
    }
    invalid.sort();
    Err(Refusal::new(GraphRule::OptionalUnsafe).with(Json::texts(&invalid)))
}
