//! Path-sensitive resource scopes of pinned workflow definitions.
use hf_domain::workflow::json::JsonObject;
use hf_domain::workflow::tree::{DependencyTree, nodes, text};
use hf_domain::workflow::vocab::MAX_NESTING_DEPTH;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceScopeFailure {
    CycleOrDepth,
    MissingDependency {
        workflow_id: String,
    },
    WorkflowExceeds {
        workflow: String,
        resources: Vec<String>,
        enclosing: String,
    },
    NodeExceeds {
        workflow: String,
        node_id: String,
        resources: Vec<String>,
        enclosing: String,
    },
}

type Scope = Option<BTreeSet<String>>;
type Context = (String, Scope, Option<String>);

struct Frame<'a> {
    context: Context,
    scope: Scope,
    label: Option<String>,
    name: String,
    nodes: Vec<&'a JsonObject>,
    next: usize,
}

fn resources(object: &JsonObject) -> BTreeSet<String> {
    object
        .get("resources")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::to_string)
        .collect()
}

fn exceeding(declared: &BTreeSet<String>, inherited: &Scope) -> Vec<String> {
    inherited.as_ref().map_or_else(Vec::new, |scope| {
        declared.difference(scope).cloned().collect()
    })
}

fn frame<'a>(
    tree: &'a DependencyTree,
    context: Context,
) -> Result<Frame<'a>, ResourceScopeFailure> {
    let (id, inherited, enclosing) = &context;
    let definition = &tree
        .entry(id)
        .ok_or_else(|| ResourceScopeFailure::MissingDependency {
            workflow_id: id.clone(),
        })?
        .definition;
    let name = definition
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or(id)
        .to_string();
    let (scope, label) = if definition.contains_key("resources") {
        let declared = resources(definition);
        let missing = exceeding(&declared, inherited);
        if !missing.is_empty() {
            return Err(ResourceScopeFailure::WorkflowExceeds {
                workflow: name,
                resources: missing,
                enclosing: enclosing.clone().unwrap_or_default(),
            });
        }
        (Some(declared), Some(name.clone()))
    } else {
        (inherited.clone(), enclosing.clone())
    };
    Ok(Frame {
        context,
        scope,
        label,
        name,
        nodes: nodes(definition, "nodes")
            .chain(nodes(definition, "on_demand_nodes"))
            .collect(),
        next: 0,
    })
}

/// Walks definitions, never cached edges. Successful completed contexts cover only
/// equal or shallower invocation depths; active paths always check cycles and depth.
/// Definitions must already have passed workflow validation. This walk checks only
/// enclosing resource declarations; it reads JSON without recursively copying it.
pub fn validate_tree_scopes(tree: &DependencyTree) -> Result<(), ResourceScopeFailure> {
    let root = (tree.root_workflow_id.clone(), None, None);
    let mut stack = Vec::from([frame(tree, root)?]);
    let mut completed: BTreeMap<Context, usize> = BTreeMap::new();
    while let Some(current) = stack.last_mut() {
        if current.next == current.nodes.len() {
            let depth = stack.len() - 1;
            if let Some(done) = stack.pop() {
                completed
                    .entry(done.context)
                    .and_modify(|seen| *seen = (*seen).max(depth))
                    .or_insert(depth);
            }
            continue;
        }
        let node = current.nodes[current.next];
        current.next += 1;
        let missing = exceeding(&resources(node), &current.scope);
        if !missing.is_empty() {
            return Err(ResourceScopeFailure::NodeExceeds {
                workflow: current.name.clone(),
                node_id: text(node, "id").to_string(),
                resources: missing,
                enclosing: current.label.clone().unwrap_or_default(),
            });
        }
        if text(node, "type") != "workflow" {
            continue;
        }
        let id = node
            .get("workflow_ref")
            .and_then(|value| value.as_object())
            .map_or("", |reference| text(reference, "workflow_id"))
            .to_string();
        let context = (id, current.scope.clone(), current.label.clone());
        let depth = stack.len();
        if depth >= MAX_NESTING_DEPTH || stack.iter().any(|item| item.context.0 == context.0) {
            return Err(ResourceScopeFailure::CycleOrDepth);
        }
        if completed.get(&context).is_some_and(|seen| depth <= *seen) {
            continue;
        }
        stack.push(frame(tree, context)?);
    }
    Ok(())
}
