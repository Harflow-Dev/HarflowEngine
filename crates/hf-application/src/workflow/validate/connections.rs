//! Connections: identity, endpoints, retry caps and flags.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::DefinitionRule;
use hf_domain::workflow::vocab::NodeType;

use super::field::{Check, identifier, refuse, refuse_for};

/// A validated connection, by node index.
pub struct Edge {
    pub id: String,
    pub source: usize,
    #[allow(dead_code)]
    pub target: usize,
    pub default: bool,
}

fn retries_valid(edge: &JsonObject) -> bool {
    match edge.get("max_retries") {
        None => true,
        Some(Json::Int(number)) => !number.is_negative(),
        Some(_) => false,
    }
}

fn position_of(ids: &[String], value: Option<&Json>) -> Option<usize> {
    let name = value?.as_str()?;
    ids.iter().position(|id| id == name)
}

/// Validates and normalizes every connection in order; returns them by node index.
pub fn validate(list: &mut [Json], ids: &[String], kinds: &[NodeType]) -> Check<Vec<Edge>> {
    let mut edges: Vec<Edge> = Vec::new();
    for item in list {
        let edge = item
            .as_object_mut()
            .ok_or_else(|| refuse(DefinitionRule::ConnectionNotObject))?;
        let id = identifier(edge.get("id"))?;
        if edges.iter().any(|seen| seen.id == id) {
            return Err(refuse_for(DefinitionRule::DuplicateConnection, &id));
        }
        let (Some(source), Some(target)) = (
            position_of(ids, edge.get("source")),
            position_of(ids, edge.get("target")),
        ) else {
            return Err(refuse_for(DefinitionRule::ConnectionUnknownNode, &id));
        };
        if kinds[source] == NodeType::End || kinds[target] == NodeType::Start {
            return Err(refuse(DefinitionRule::EndStartConnection));
        }
        if !retries_valid(edge) {
            return Err(refuse_for(DefinitionRule::MaxRetriesInvalid, &id));
        }
        edge.set_default("condition", Json::text(""));
        edge.set_default("default", Json::Bool(false));
        edge.set_default("backward", Json::Bool(false));
        let flags = edge.get("condition").and_then(Json::as_str).is_some()
            && edge.get("default").and_then(Json::as_bool).is_some()
            && edge.get("backward").and_then(Json::as_bool).is_some();
        if !flags {
            return Err(refuse_for(DefinitionRule::ConnectionFlagsInvalid, &id));
        }
        let default = edge.get("default").and_then(Json::as_bool).unwrap_or(false);
        edges.push(Edge {
            id,
            source,
            target,
            default,
        });
    }
    Ok(edges)
}

/// Every node but End needs an outgoing connection; at most one may be the default.
pub fn check_outgoing(ids: &[String], kinds: &[NodeType], edges: &[Edge]) -> Check<()> {
    for (index, id) in ids.iter().enumerate() {
        let leaving: Vec<&Edge> = edges.iter().filter(|edge| edge.source == index).collect();
        if kinds[index] != NodeType::End && leaving.is_empty() {
            return Err(refuse_for(DefinitionRule::NoOutgoingConnection, id));
        }
        if leaving.iter().filter(|edge| edge.default).count() > 1 {
            return Err(refuse_for(DefinitionRule::MultipleDefaults, id));
        }
    }
    Ok(())
}
