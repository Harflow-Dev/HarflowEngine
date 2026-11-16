//! Graph rules over a definition's nodes and connections, as iterative algorithms over
//! node indices. Ties are broken in node-list order, never by hash order.
use super::vocab::NodeType;

mod bits;
pub mod dominance;
pub mod optional;
pub mod parallel;

pub use bits::BitSet;

/// A connected node as the graph rules see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub kind: NodeType,
    /// The index into the parallel groups, for a boundary node.
    pub group: Option<usize>,
    pub optional: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    pub source: usize,
    pub target: usize,
    pub backward: bool,
}

/// Nodes and connections in definition order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    outgoing: Vec<Vec<usize>>,
}

impl Graph {
    pub fn new(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Self {
        let mut outgoing: Vec<Vec<usize>> = nodes.iter().map(|_| Vec::new()).collect();
        for (index, edge) in edges.iter().enumerate() {
            outgoing[edge.source].push(index);
        }
        Self {
            nodes,
            edges,
            outgoing,
        }
    }

    /// The indices of the connections leaving a node, in definition order.
    pub fn outgoing(&self, node: usize) -> &[usize] {
        &self.outgoing[node]
    }

    /// The indices of the forward connections leaving a node.
    pub fn forward_edges(&self, node: usize) -> Vec<usize> {
        self.outgoing[node]
            .iter()
            .copied()
            .filter(|edge| !self.edges[*edge].backward)
            .collect()
    }

    /// The nodes reached over forward connections from `start`, not passing `stop`.
    pub fn forward_reachable(&self, start: usize, stop: Option<usize>) -> BitSet {
        let mut reached = BitSet::empty(self.nodes.len());
        let mut pending = vec_of(start);
        while let Some(node) = pending.pop() {
            if Some(node) == stop || reached.contains(node) {
                continue;
            }
            reached.insert(node);
            for edge in &self.outgoing[node] {
                if !self.edges[*edge].backward {
                    pending.push(self.edges[*edge].target);
                }
            }
        }
        reached
    }

    /// For each node the set of nodes every forward path from it passes through, itself
    /// included. Requires an acyclic forward graph.
    pub fn postdominators(&self) -> Vec<BitSet> {
        let count = self.nodes.len();
        let mut remaining: Vec<usize> = (0..count)
            .map(|node| self.forward_edges(node).len())
            .collect();
        let mut predecessors: Vec<Vec<usize>> = (0..count).map(|_| Vec::new()).collect();
        for edge in self.edges.iter().filter(|edge| !edge.backward) {
            predecessors[edge.target].push(edge.source);
        }
        let mut sets: Vec<BitSet> = (0..count).map(|_| BitSet::empty(count)).collect();
        let mut ready: Vec<usize> = (0..count).filter(|node| remaining[*node] == 0).collect();
        while let Some(node) = ready.pop() {
            let mut shared: Option<BitSet> = None;
            for edge in self.forward_edges(node) {
                let other = &sets[self.edges[edge].target];
                match shared.as_mut() {
                    Some(set) => set.intersect_with(other),
                    None => shared = Some(other.clone()),
                }
            }
            let mut set = shared.unwrap_or_else(|| BitSet::empty(count));
            set.insert(node);
            sets[node] = set;
            for predecessor in predecessors[node].clone() {
                remaining[predecessor] -= 1;
                if remaining[predecessor] == 0 {
                    ready.push(predecessor);
                }
            }
        }
        sets
    }

    /// The one node closest to all of `targets` among those every path from them reaches,
    /// or `None` when there is none or more than one.
    pub fn closest_convergence(
        &self,
        postdominators: &[BitSet],
        targets: &[usize],
    ) -> Option<usize> {
        let mut common = postdominators[*targets.first()?].clone();
        for target in targets {
            common.intersect_with(&postdominators[*target]);
        }
        let mut closest = None;
        for candidate in common.iter() {
            if common.is_subset(&postdominators[candidate]) {
                if closest.is_some() {
                    return None;
                }
                closest = Some(candidate);
            }
        }
        closest
    }
}

fn vec_of(node: usize) -> Vec<usize> {
    Vec::from([node])
}
