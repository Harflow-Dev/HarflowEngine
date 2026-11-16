//! Run and node resource declarations.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::name::is_resource_name;
use hf_domain::workflow::rule_shape::OnDemandRule;

use super::field::{Check, refuse, refuse_for};

const MAX_NAMES: usize = 16;

/// `resources.validate_names`: sorted, unique, valid names. A string, an object or a
/// scalar is refused (Python accepted an object as its keys).
fn names(value: &Json) -> Check<Vec<String>> {
    let items = match value {
        Json::Str(_) => return Err(refuse(OnDemandRule::ResourceNamesSingleString)),
        Json::Array(items) => items,
        _ => return Err(refuse(OnDemandRule::ResourceNamesNotList)),
    };
    if items.is_empty() || items.len() > MAX_NAMES {
        return Err(refuse(OnDemandRule::ResourceCount)
            .with(Json::int(i64::try_from(items.len()).unwrap_or(i64::MAX))));
    }
    let mut listed = Vec::new();
    for item in items {
        match item.as_str() {
            Some(name) if is_resource_name(name) => listed.push(name.to_string()),
            _ => return Err(refuse(OnDemandRule::ResourceNameInvalid).with(item.clone())),
        }
    }
    let mut sorted = listed.clone();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != listed.len() {
        return Err(refuse(OnDemandRule::ResourceDuplicate));
    }
    Ok(sorted)
}

fn node_resources(list: &mut [Json], declared: Option<&[String]>, on_demand: bool) -> Check<()> {
    for item in list {
        let Some(node) = item.as_object_mut() else {
            continue;
        };
        let Some(value) = node.get("resources").cloned() else {
            continue;
        };
        let id = node
            .get("id")
            .and_then(Json::as_str)
            .unwrap_or("")
            .to_string();
        let kind = node.get("type").and_then(Json::as_str).unwrap_or("agent");
        if !on_demand && kind != "agent" {
            return Err(refuse_for(OnDemandRule::ResourcesOnlyAgents, &id));
        }
        let listed = names(&value)?;
        let mode = node
            .get("execution_mode")
            .and_then(Json::as_str)
            .unwrap_or("headless");
        if mode == "prefer_subagent" {
            return Err(refuse_for(OnDemandRule::ResourcePreferSubagent, &id));
        }
        if declared.is_some_and(|run| listed.iter().any(|name| !run.contains(name))) {
            return Err(refuse_for(OnDemandRule::ResourceSubset, &id));
        }
        node.insert("resources", Json::texts(&listed));
    }
    Ok(())
}

/// `workflow_resources.validate_definition`: normalizes run and node resources and checks
/// that each node's resources are a subset of the run's. The connected nodes are passed
/// apart from `definition`; its on-demand nodes are read from it.
pub fn validate_definition(definition: &mut JsonObject, nodes: &mut [Json]) -> Check<()> {
    let declared = match definition.get("resources") {
        Some(value) => Some(names(value)?),
        None => None,
    };
    node_resources(nodes, declared.as_deref(), false)?;
    if let Some(Json::Array(list)) = definition.get_mut("on_demand_nodes") {
        node_resources(list, declared.as_deref(), true)?;
    }
    if let Some(run) = declared {
        definition.insert("resources", Json::texts(&run));
    }
    Ok(())
}
