//! A resolved dependency tree: every workflow pinned by identity, in insertion order.
use super::json::{Json, JsonObject};
use super::vocab::Freedom;

/// A pinned workflow.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeEntry {
    pub name: String,
    pub revision: Json,
    pub definition_sha256: String,
    pub definition: JsonObject,
}

/// A reference from one pinned workflow to another.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeEdge {
    pub from: String,
    pub node_id: String,
    pub to: String,
    pub orchestrator_mode: String,
}

/// What a pinned definition can do; never widened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessSummary {
    pub max_freedom: Freedom,
    pub network: bool,
    pub backends: Vec<String>,
    pub depth: usize,
}

/// The pinned workflows, their edges and access summaries. Workflows and access keep the
/// order in which they were pinned.
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyTree {
    pub root_workflow_id: String,
    pub workflows: Vec<(String, TreeEntry)>,
    pub edges: Vec<TreeEdge>,
    pub access: Vec<(String, AccessSummary)>,
}

impl DependencyTree {
    pub fn entry(&self, workflow_id: &str) -> Option<&TreeEntry> {
        self.workflows
            .iter()
            .find(|(id, _)| id == workflow_id)
            .map(|(_, entry)| entry)
    }

    pub fn access_of(&self, workflow_id: &str) -> Option<&AccessSummary> {
        self.access
            .iter()
            .find(|(id, _)| id == workflow_id)
            .map(|(_, summary)| summary)
    }
}

/// Saved definitions with a direct reference to an identity, sorted and unique.
pub fn referencing_definitions(definitions: &[JsonObject], workflow_id: &str) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for definition in definitions {
        if nodes(definition, "nodes").any(|node| {
            text(node, "type") == "workflow"
                && node
                    .get("workflow_ref")
                    .and_then(Json::as_object)
                    .is_some_and(|reference| text(reference, "workflow_id") == workflow_id)
        }) {
            names.insert(text(definition, "name").to_string());
        }
    }
    names.into_iter().collect()
}

pub fn nodes<'a>(definition: &'a JsonObject, key: &str) -> impl Iterator<Item = &'a JsonObject> {
    definition
        .get(key)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_object)
}

pub fn text<'a>(object: &'a JsonObject, key: &str) -> &'a str {
    object.get(key).and_then(Json::as_str).unwrap_or("")
}

/// Access of a qualified definition, including on-demand agents and direct fallbacks.
pub fn access_summary(definition: &JsonObject, depth: usize) -> AccessSummary {
    use super::vocab::Role;
    let mut summary = AccessSummary {
        max_freedom: Freedom::ReadOnly,
        network: false,
        backends: Vec::new(),
        depth,
    };
    let mut backends = std::collections::BTreeSet::new();
    if let Some(orchestrator) = definition.get("orchestrator").and_then(Json::as_object) {
        add_backend(&mut backends, orchestrator);
    }
    let agents = nodes(definition, "nodes")
        .filter(|node| text(node, "type") == "agent")
        .chain(nodes(definition, "on_demand_nodes"));
    for node in agents {
        let role = Role::parse(text(node, "role")).unwrap_or(Role::Task);
        let freedom = Freedom::parse(text(node, "freedom")).unwrap_or(role.default_freedom());
        summary.max_freedom = summary.max_freedom.max(freedom);
        summary.network |= node.get("network") == Some(&Json::Bool(true));
        if let Some(agent) = node.get("agent").and_then(Json::as_object) {
            add_backend(&mut backends, agent);
            for fallback in nodes(agent, "fallbacks") {
                add_backend(&mut backends, fallback);
            }
        }
    }
    summary.backends = backends.into_iter().collect();
    summary
}

fn add_backend(backends: &mut std::collections::BTreeSet<String>, candidate: &JsonObject) {
    let backend = text(candidate, "backend");
    if !backend.is_empty() {
        backends.insert(backend.to_string());
    }
}

impl DependencyTree {
    /// Reachable subtree, retaining parent insertion order for every collection.
    pub fn subtree(&self, workflow_id: &str) -> Option<Self> {
        self.entry(workflow_id)?;
        let mut ids = std::collections::BTreeSet::from([workflow_id.to_string()]);
        let mut frontier = Vec::from([workflow_id.to_string()]);
        while let Some(current) = frontier.pop() {
            for edge in &self.edges {
                if edge.from == current && ids.insert(edge.to.clone()) {
                    frontier.push(edge.to.clone());
                }
            }
        }
        Some(Self {
            root_workflow_id: workflow_id.to_string(),
            workflows: self
                .workflows
                .iter()
                .filter(|(id, _)| ids.contains(id))
                .cloned()
                .collect(),
            edges: self
                .edges
                .iter()
                .filter(|edge| ids.contains(&edge.from))
                .cloned()
                .collect(),
            access: self
                .access
                .iter()
                .filter(|(id, _)| ids.contains(id))
                .cloned()
                .collect(),
        })
    }
}
