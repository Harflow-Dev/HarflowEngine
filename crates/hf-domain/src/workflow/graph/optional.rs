//! Optional nodes: a safe parallel branch with a required sibling and no required
//! successor before convergence.
use std::collections::HashMap;

use super::{BitSet, Graph};
use crate::workflow::json::Json;
use crate::workflow::refusal::Refusal;
use crate::workflow::rule_graph::GraphRule;
use crate::workflow::vocab::NodeType;

/// What the check needs of the forward region from one branch entry up to a join.
struct Region {
    members: BitSet,
    executable: usize,
    /// The optional executable nodes inside the region.
    optional: Vec<usize>,
}

impl Region {
    /// Python's `_required_region`: executable nodes, none of them optional.
    fn required(&self) -> bool {
        self.executable > 0 && self.optional.is_empty()
    }

    /// Whether every executable node of the region is optional.
    fn only_optional(&self) -> bool {
        self.executable == self.optional.len()
    }
}

/// Memoized work of one validation, keyed by branch entry and join.
struct Memo<'a> {
    graph: &'a Graph,
    postdominators: &'a [BitSet],
    joins: HashMap<(usize, usize), usize>,
    regions: HashMap<(usize, usize), Region>,
    suffixes: HashMap<(usize, usize), bool>,
}

impl Memo<'_> {
    fn join(&mut self, left: usize, right: usize) -> Result<usize, Refusal> {
        let key = (left.min(right), left.max(right));
        if let Some(found) = self.joins.get(&key) {
            return Ok(*found);
        }
        let found = selection_join(self.graph, self.postdominators, &[left, right])?;
        self.joins.insert(key, found);
        Ok(found)
    }

    fn ensure(&mut self, target: usize, join: usize) {
        let graph = self.graph;
        self.regions.entry((target, join)).or_insert_with(|| {
            let members = graph.forward_reachable(target, Some(join));
            let mut executable = 0;
            let mut optional = Vec::new();
            for node in members.iter() {
                let item = &graph.nodes[node];
                if item.kind.is_executable() {
                    executable += 1;
                    if item.optional {
                        optional.push(node);
                    }
                }
            }
            Region {
                members,
                executable,
                optional,
            }
        });
    }

    /// `_optional_suffix`: no required executable node between `node` and `join`.
    fn suffix(&mut self, node: usize, join: usize) -> bool {
        if let Some(found) = self.suffixes.get(&(node, join)) {
            return *found;
        }
        self.ensure(node, join);
        let found = self
            .regions
            .get(&(node, join))
            .is_some_and(|region| region.members.contains(node) && region.only_optional());
        self.suffixes.insert((node, join), found);
        found
    }
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

/// `validate_optional_nodes`. In explicit routing only parallel starts split. A pair of
/// connections to the same target is a no-op in the reference, so each split's targets
/// are deduplicated before pairing; results are memoized per target and join.
pub fn validate(graph: &Graph, postdominators: &[BitSet], explicit: bool) -> Result<(), Refusal> {
    let optional: Vec<usize> = (0..graph.nodes.len())
        .filter(|index| graph.nodes[*index].optional && graph.nodes[*index].kind.is_executable())
        .collect();
    if optional.is_empty() {
        return Ok(());
    }
    let mut valid = BitSet::empty(graph.nodes.len());
    let mut memo = Memo {
        graph,
        postdominators,
        joins: HashMap::new(),
        regions: HashMap::new(),
        suffixes: HashMap::new(),
    };
    for split in 0..graph.nodes.len() {
        if explicit && graph.nodes[split].kind != NodeType::ParallelStart {
            continue;
        }
        let mut targets: Vec<usize> = Vec::new();
        for edge in graph.forward_edges(split) {
            let target = graph.edges[edge].target;
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        for (index, left) in targets.iter().enumerate() {
            for right in &targets[..index] {
                pair(&mut memo, &mut valid, *left, *right)?;
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

fn pair(memo: &mut Memo<'_>, valid: &mut BitSet, left: usize, right: usize) -> Result<(), Refusal> {
    let join = memo.join(left, right)?;
    memo.ensure(left, join);
    memo.ensure(right, join);
    let (Some(one), Some(two)) = (
        memo.regions.get(&(left, join)),
        memo.regions.get(&(right, join)),
    ) else {
        return Ok(());
    };
    if one.members.intersects(&two.members) {
        return Ok(());
    }
    let mut chosen: Vec<usize> = Vec::new();
    if two.required() {
        chosen.extend(&one.optional);
    }
    if one.required() {
        chosen.extend(&two.optional);
    }
    for node in chosen {
        if !valid.contains(node) && memo.suffix(node, join) {
            valid.insert(node);
        }
    }
    Ok(())
}
