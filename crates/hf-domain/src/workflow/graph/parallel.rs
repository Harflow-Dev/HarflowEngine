//! Parallel boundaries: structured single-entry regions, including nested pairs.
use super::{BitSet, Graph};
use crate::workflow::json::Json;
use crate::workflow::refusal::Refusal;
use crate::workflow::rule_graph::GraphRule;
use crate::workflow::vocab::NodeType;

/// A parallel group as declared by its boundary nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Group {
    pub id: String,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

fn text(graph: &Graph, node: usize) -> Json {
    Json::text(&graph.nodes[node].id)
}

fn edge_id(graph: &Graph, edge: usize) -> Json {
    Json::text(&graph.edges[edge].id)
}

/// What is wrong inside a branch.
enum Inside {
    ReachesEnd,
    ImproperNesting,
}

fn nested_inside(graph: &Graph, groups: &[Group], region: &BitSet) -> Result<(), Inside> {
    for node in region.members() {
        let boundary = graph.nodes[node].kind;
        if boundary == NodeType::End {
            return Err(Inside::ReachesEnd);
        }
        let partner = match (boundary, graph.nodes[node].group) {
            (NodeType::ParallelStart | NodeType::ParallelEnd, Some(group)) => &groups[group],
            _ => continue,
        };
        let outside = [partner.start, partner.end]
            .into_iter()
            .flatten()
            .any(|member| !region.contains(member));
        if outside {
            return Err(Inside::ImproperNesting);
        }
    }
    Ok(())
}

fn check_edges(
    graph: &Graph,
    group: &Group,
    bounds: (usize, usize),
    entry_edges: &[usize],
    region: &BitSet,
) -> Result<(), Refusal> {
    let (start, end) = bounds;
    for (index, edge) in graph.edges.iter().enumerate() {
        let (source, target) = (edge.source, edge.target);
        let entered = region.contains(target) && !region.contains(source);
        if entered && !(source == start && entry_edges.contains(&index)) {
            return Err(Refusal::new(GraphRule::ParallelExternalEntry)
                .with_text(&group.id)
                .with(edge_id(graph, index)));
        }
        if region.contains(source) && !region.contains(target) && target != end {
            return Err(Refusal::new(GraphRule::ParallelEscape)
                .with_text(&group.id)
                .with(edge_id(graph, index)));
        }
        if source == end && region.contains(target) {
            return Err(Refusal::new(GraphRule::ParallelRetryEntry).with(edge_id(graph, index)));
        }
    }
    Ok(())
}

fn check_group(
    graph: &Graph,
    groups: &[Group],
    group: &Group,
    postdominators: &[BitSet],
) -> Result<(usize, usize), Refusal> {
    let (Some(start), Some(end)) = (group.start, group.end) else {
        return Err(Refusal::new(GraphRule::ParallelGroupIncomplete).with_text(&group.id));
    };
    let entry_edges = graph.forward_edges(start);
    if entry_edges.len() < 2 || entry_edges.len() != graph.outgoing(start).len() {
        return Err(Refusal::new(GraphRule::ParallelStartBranches).with(text(graph, start)));
    }
    if !postdominators[start].contains(end) {
        return Err(Refusal::new(GraphRule::ParallelBranchMissesEnd)
            .with(text(graph, start))
            .with(text(graph, end)));
    }
    let regions: Vec<BitSet> = entry_edges
        .iter()
        .map(|edge| graph.forward_reachable(graph.edges[*edge].target, Some(end)))
        .collect();
    if regions.iter().any(BitSet::is_empty) {
        return Err(Refusal::new(GraphRule::ParallelEmptyBranch).with(text(graph, start)));
    }
    for (index, region) in regions.iter().enumerate() {
        if regions[..index]
            .iter()
            .any(|previous| region.intersects(previous))
        {
            return Err(Refusal::new(GraphRule::ParallelOverlap).with_text(&group.id));
        }
        match nested_inside(graph, groups, region) {
            Err(Inside::ReachesEnd) => {
                return Err(Refusal::new(GraphRule::ParallelReachesEnd).with_text(&group.id));
            }
            Err(Inside::ImproperNesting) => {
                return Err(Refusal::new(GraphRule::ParallelImproperNesting).with_text(&group.id));
            }
            Ok(()) => {}
        }
        check_edges(graph, group, (start, end), &entry_edges, region)?;
    }
    for edge in &graph.edges {
        if edge.target == end && !regions.iter().any(|region| region.contains(edge.source)) {
            return Err(Refusal::new(GraphRule::ParallelEndExternalEntry).with(text(graph, end)));
        }
    }
    Ok((start, end))
}

/// Checks every group in declaration order and returns each group's `(start, end)` for the
/// start's `join_id`. `postdominators` come from the finished forward graph.
pub fn check_groups(
    graph: &Graph,
    groups: &[Group],
    postdominators: &[BitSet],
) -> Result<Vec<(usize, usize)>, Refusal> {
    groups
        .iter()
        .map(|group| check_group(graph, groups, group, postdominators))
        .collect()
}
