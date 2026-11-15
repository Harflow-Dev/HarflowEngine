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
