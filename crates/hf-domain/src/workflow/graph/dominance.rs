//! Reachability, natural loops and forward paths to End.
use super::{BitSet, Graph};
use crate::workflow::json::Json;
use crate::workflow::refusal::Refusal;
use crate::workflow::rule_graph::GraphRule;
use crate::workflow::vocab::NodeType;

fn reachable(graph: &Graph, start: usize) -> BitSet {
    let mut reached = BitSet::empty(graph.nodes.len());
    let mut pending = Vec::from([start]);
    reached.insert(start);
    while let Some(node) = pending.pop() {
        for edge in graph.outgoing(node) {
            let target = graph.edges[*edge].target;
            if !reached.contains(target) {
                reached.insert(target);
                pending.push(target);
            }
        }
    }
    reached
}

/// For each node, the nodes that dominate it: every path from `start` passes through them.
fn dominators(graph: &Graph, start: usize) -> Vec<BitSet> {
    let count = graph.nodes.len();
    let mut predecessors: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
    for edge in &graph.edges {
        predecessors[edge.target].push(edge.source);
    }
    let mut sets: Vec<BitSet> = (0..count).map(|_| BitSet::full(count)).collect();
    sets[start] = BitSet::empty(count);
    sets[start].insert(start);
    let mut changed = true;
    while changed {
        changed = false;
        for node in (0..count).filter(|node| *node != start) {
            let mut shared = BitSet::full(count);
            for predecessor in &predecessors[node] {
                shared.intersect_with(&sets[*predecessor]);
            }
            shared.insert(node);
            if shared != sets[node] {
                sets[node] = shared;
                changed = true;
            }
        }
    }
    sets
}

/// Whether a forward walk from `start` meets a node on its own path.
fn has_forward_cycle(graph: &Graph, start: usize) -> bool {
    // 1 while on the path, 2 once finished.
    let mut state: Vec<u8> = (0..graph.nodes.len()).map(|_| 0).collect();
    let mut stack: Vec<(usize, usize)> = Vec::from([(start, 0)]);
    state[start] = 1;
    while let Some((node, cursor)) = stack.pop() {
        let forward = graph.forward_edges(node);
        if cursor == forward.len() {
            state[node] = 2;
            continue;
        }
        stack.push((node, cursor + 1));
        let target = graph.edges[forward[cursor]].target;
        match state[target] {
            1 => return true,
            0 => {
                state[target] = 1;
                stack.push((target, 0));
            }
            _ => {}
        }
    }
    false
}

fn forward_visited(graph: &Graph, start: usize) -> BitSet {
    graph.forward_reachable(start, None)
}

/// The nodes with a forward path to some End.
fn can_end(graph: &Graph) -> BitSet {
    let mut ends = BitSet::empty(graph.nodes.len());
    for (index, node) in graph.nodes.iter().enumerate() {
        if node.kind == NodeType::End {
            ends.insert(index);
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for index in 0..graph.nodes.len() {
            let reaches = !ends.contains(index)
                && graph
                    .forward_edges(index)
                    .iter()
                    .any(|edge| ends.contains(graph.edges[*edge].target));
            if reaches {
                ends.insert(index);
                changed = true;
            }
        }
    }
    ends
}

/// Marks every connection that returns to a node dominating its source as `backward`,
/// then checks that the forward graph is acyclic, that every node is forward-reachable
/// from `start` and that every node has a forward path to End.
pub fn analyze(graph: &mut Graph, start: usize) -> Result<(), Refusal> {
    let count = graph.nodes.len();
    if reachable(graph, start).members().len() != count {
        return Err(Refusal::new(GraphRule::UnreachableFromStart));
    }
    let sets = dominators(graph, start);
    for edge in &mut graph.edges {
        edge.backward = sets[edge.source].contains(edge.target);
    }
    if has_forward_cycle(graph, start) {
        return Err(Refusal::new(GraphRule::AmbiguousLoop));
    }
    if forward_visited(graph, start).members().len() != count {
        return Err(Refusal::new(GraphRule::NotForwardReachable));
    }
    let ending = can_end(graph);
    let mut missing: Vec<String> = (0..count)
        .filter(|index| !ending.contains(*index))
        .map(|index| graph.nodes[index].id.clone())
        .collect();
    if !missing.is_empty() {
        missing.sort();
        return Err(Refusal::new(GraphRule::NoForwardPathToEnd).with(Json::texts(&missing)));
    }
    Ok(())
}
