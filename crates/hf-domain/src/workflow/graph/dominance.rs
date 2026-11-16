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

/// The nodes reachable from `start` in reverse postorder of a depth-first walk over every
/// connection, so each node comes after its predecessors except across loops.
fn reverse_postorder(graph: &Graph, start: usize) -> Vec<usize> {
    let mut seen = BitSet::empty(graph.nodes.len());
    let mut order: Vec<usize> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::from([(start, 0)]);
    seen.insert(start);
    while let Some((node, cursor)) = stack.pop() {
        let leaving = graph.outgoing(node);
        let Some(edge) = leaving.get(cursor) else {
            order.push(node);
            continue;
        };
        stack.push((node, cursor + 1));
        let target = graph.edges[*edge].target;
        if !seen.contains(target) {
            seen.insert(target);
            stack.push((target, 0));
        }
    }
    order.reverse();
    order
}

/// For each node, the nodes that dominate it: every path from `start` passes through them.
/// Passes run in reverse postorder whatever order the nodes are listed in: structured graphs
/// reach the greatest fixed point in a few passes, any graph in at most O(N) passes. The
/// result does not depend on the order.
fn dominators(graph: &Graph, start: usize) -> Vec<BitSet> {
    let count = graph.nodes.len();
    let mut predecessors: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
    for edge in &graph.edges {
        predecessors[edge.target].push(edge.source);
    }
    let order = reverse_postorder(graph, start);
    let mut sets: Vec<BitSet> = (0..count).map(|_| BitSet::full(count)).collect();
    sets[start] = BitSet::empty(count);
    sets[start].insert(start);
    let mut changed = true;
    while changed {
        changed = false;
        for node in order.iter().copied().filter(|node| *node != start) {
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
        let leaving = graph.outgoing(node);
        let Some(edge) = leaving.get(cursor) else {
            state[node] = 2;
            continue;
        };
        stack.push((node, cursor + 1));
        if graph.edges[*edge].backward {
            continue;
        }
        let target = graph.edges[*edge].target;
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

/// The nodes with a forward path to some End, found by walking forward connections
/// backwards from every End once.
fn can_end(graph: &Graph) -> BitSet {
    let count = graph.nodes.len();
    let mut predecessors: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
    for edge in graph.edges.iter().filter(|edge| !edge.backward) {
        predecessors[edge.target].push(edge.source);
    }
    let mut ends = BitSet::empty(count);
    let mut pending: Vec<usize> = Vec::new();
    for (index, node) in graph.nodes.iter().enumerate() {
        if node.kind == NodeType::End {
            ends.insert(index);
            pending.push(index);
        }
    }
    while let Some(node) = pending.pop() {
        for predecessor in &predecessors[node] {
            if !ends.contains(*predecessor) {
                ends.insert(*predecessor);
                pending.push(*predecessor);
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
