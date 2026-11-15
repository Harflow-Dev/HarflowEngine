//! `validate_definition`: normalizes a candidate definition and refuses what cannot be
//! saved. The checks run in Python's order, which also fixes the key order of the result.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::{DefinitionRule, NodeRule};
use hf_domain::workflow::vocab::{MAX_CONNECTIONS, MAX_NODES, NodeType};

use super::ports::{BackendCatalog, PathResolver};
use field::{Check, is_one_of, positive, refuse};

mod agent;
mod candidate;
mod candidate_backend;
mod connections;
pub mod field;
mod graph;
pub mod migrate;
mod node;
mod on_demand;
mod resources;

/// Validation over the backend catalog and path resolver of the host.
pub struct Validator<'a> {
    pub catalog: &'a dyn BackendCatalog,
    pub resolver: &'a dyn PathResolver,
}

fn take_list(d: &mut JsonObject, key: &str) -> Option<Vec<Json>> {
    match d.get_mut(key) {
        Some(Json::Array(items)) => Some(std::mem::take(items)),
        _ => None,
    }
}

impl<'a> Validator<'a> {
    pub fn new(catalog: &'a dyn BackendCatalog, resolver: &'a dyn PathResolver) -> Self {
        Self { catalog, resolver }
    }

    /// Python's `validate_definition`, without stored qualification.
    pub fn normalize(&self, raw: &Json) -> Check<JsonObject> {
        let Json::Object(source) = raw else {
            return Err(refuse(DefinitionRule::NotObject));
        };
        let mut d = migrate::migrate_orchestrator_instructions(source)?;
        let explicit = self.top_level(&mut d)?;
        let had_connections = d.contains_key("connections");
        let connections = match d.get("connections") {
            None | Some(Json::Array(_)) => take_list(&mut d, "connections"),
            Some(_) => None,
        };
        let (Some(mut nodes), true) = (
            take_list(&mut d, "nodes"),
            connections.is_some() || !had_connections,
        ) else {
            return Err(refuse(DefinitionRule::NodesAndConnectionsRequired));
        };
        let mut connections = connections.unwrap_or_default();
        self.check_bounds(&d, &nodes, &connections)?;
        let (ids, kinds) = self.nodes_loop(&mut nodes, explicit)?;
        let on_demand = d
            .get("on_demand_nodes")
            .cloned()
            .unwrap_or(Json::Array(Vec::new()));
        d.insert(
            "on_demand_nodes",
            Json::Array(self.on_demand_nodes(&on_demand, &ids)?),
        );
        resources::validate_definition(&mut d, &mut nodes)?;
        let starts: Vec<usize> = (0..kinds.len())
            .filter(|i| kinds[*i] == NodeType::Start)
            .collect();
        if starts.len() != 1 || !kinds.contains(&NodeType::End) {
            return Err(refuse(DefinitionRule::StartEndRequired));
        }
        let edges = connections::validate(&mut connections, &ids, &kinds)?;
        connections::check_outgoing(&ids, &kinds, &edges)?;
        let info = graph::Nodes {
            ids,
            kinds,
            start: starts[0],
        };
        graph::validate(explicit, &mut nodes, &mut connections, &info, &edges)?;
        d.insert("nodes", Json::Array(nodes));
        if had_connections {
            d.insert("connections", Json::Array(connections));
        }
        Ok(d)
    }

    /// Everything before the node lists; returns whether routing is explicit.
    fn top_level(&self, d: &mut JsonObject) -> Check<bool> {
        let name = d
            .get("name")
            .and_then(Json::as_str)
            .filter(|text| hf_domain::workflow::name::is_workflow_name(text));
        let Some(name) = name.map(str::to_string) else {
            return Err(refuse(DefinitionRule::InvalidWorkflowName));
        };
        d.insert("name", Json::Str(name));
        d.insert("schema_version", Json::int(1));
        let delivery = d
            .get("context_delivery")
            .cloned()
            .unwrap_or(Json::text("optimized_v1"));
        if !is_one_of(Some(&delivery), &["legacy", "optimized_v1"]) {
            return Err(refuse(DefinitionRule::InvalidContextDelivery));
        }
        let explicit = d.get("routing_mode").and_then(Json::as_str) == Some("explicit");
        if !explicit && d.get("routing_mode").is_some_and(|mode| !mode.is_null()) {
            return Err(refuse(DefinitionRule::InvalidRoutingMode));
        }
        d.set_default("description", Json::text(""));
        if d.get("description").and_then(Json::as_str).is_none() {
            return Err(refuse(DefinitionRule::DescriptionNotText));
        }
        self.orchestrator(d)?;
        d.insert(
            "max_on_demand_calls",
            positive(d, "max_on_demand_calls", 5, 50)?,
        );
        d.insert("max_parallel", positive(d, "max_parallel", 4, 64)?);
        d.insert(
            "max_transitions",
            positive(d, "max_transitions", 100, 10000)?,
        );
        d.insert(
            "max_decision_attempts",
            positive(d, "max_decision_attempts", 3, 10)?,
        );
        d.insert("max_inspections", positive(d, "max_inspections", 20, 1000)?);
        Ok(explicit)
    }

    fn orchestrator(&self, d: &mut JsonObject) -> Check<()> {
        let Some(Json::Object(orchestrator)) = d.get("orchestrator") else {
            return Err(refuse(DefinitionRule::OrchestratorNotObject));
        };
        let instructions = match orchestrator.get("instructions") {
            None => Json::text(""),
            Some(text @ Json::Str(_)) => text.clone(),
            Some(_) => return Err(refuse(DefinitionRule::OrchestratorInstructionsNotText)),
        };
        let mut rest = orchestrator.clone();
        rest.remove("instructions");
        let mut checked = self.candidate(&Json::Object(rest), false)?;
        checked.insert("instructions", instructions);
        d.insert("orchestrator", Json::Object(checked));
        Ok(())
    }

    fn check_bounds(&self, d: &JsonObject, nodes: &[Json], connections: &[Json]) -> Check<()> {
        if nodes.is_empty() {
            return Err(refuse(DefinitionRule::NodesAndConnectionsRequired));
        }
        let extra = d
            .get("on_demand_nodes")
            .and_then(Json::as_array)
            .map_or(0, Vec::len);
        if nodes.len() + extra > MAX_NODES {
            return Err(refuse(DefinitionRule::TooManyNodes));
        }
        if connections.len() > MAX_CONNECTIONS {
            return Err(refuse(DefinitionRule::TooManyConnections));
        }
        Ok(())
    }

    fn nodes_loop(
        &self,
        nodes: &mut [Json],
        explicit: bool,
    ) -> Check<(Vec<String>, Vec<NodeType>)> {
        let mut ids: Vec<String> = Vec::new();
        let mut kinds = Vec::new();
        for (index, item) in nodes.iter_mut().enumerate() {
            let map = item
                .as_object_mut()
                .ok_or_else(|| refuse(NodeRule::NodeNotObject))?;
            let checked = self.connected_node(index, map, explicit, &ids)?;
            ids.push(checked.id);
            kinds.push(checked.kind);
        }
        Ok((ids, kinds))
    }
}
