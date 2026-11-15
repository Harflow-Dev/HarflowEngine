//! Qualification of a stored definition: the types a saved document must hold, so that no
//! source is written that cannot be read back. Graph semantics are not migrated here.
use super::json::{Json, JsonObject};
use super::name::is_workflow_name;
use super::observe::{MAX_STORED, is_absolute};
use super::refusal::DocumentReason;
use super::text::is_blank;

/// The exact kind of a JSON value, as Python's `type(value)` tells them apart.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Str,
    Bool,
    Int,
    Float,
    Null,
    Dict,
    List,
}

fn kind_of(value: &Json) -> Kind {
    match value {
        Json::Null => Kind::Null,
        Json::Bool(_) => Kind::Bool,
        Json::Int(_) => Kind::Int,
        Json::Float(_) => Kind::Float,
        Json::Str(_) => Kind::Str,
        Json::Array(_) => Kind::List,
        Json::Object(_) => Kind::Dict,
    }
}

type Shape = &'static [(&'static str, &'static [Kind])];

const STR: &[Kind] = &[Kind::Str];
const BOOL: &[Kind] = &[Kind::Bool];
const INT: &[Kind] = &[Kind::Int];
const NUMBER: &[Kind] = &[Kind::Int, Kind::Float];
const DICT: &[Kind] = &[Kind::Dict];
const LIST: &[Kind] = &[Kind::List];
const STR_OR_NULL: &[Kind] = &[Kind::Str, Kind::Null];
const INT_OR_NULL: &[Kind] = &[Kind::Int, Kind::Null];
const NUMBER_OR_NULL: &[Kind] = &[Kind::Int, Kind::Float, Kind::Null];
const BOOL_OR_NULL: &[Kind] = &[Kind::Bool, Kind::Null];

const CANDIDATE: Shape = &[
    ("backend", STR),
    ("model", STR_OR_NULL),
    ("reasoning_effort", STR_OR_NULL),
    ("max_turns", INT_OR_NULL),
    ("background_idle_seconds", NUMBER_OR_NULL),
    ("caller_approvals", BOOL),
    ("caller_write_approvals", BOOL),
    ("allowed_commands", LIST),
    ("observe_repos", LIST),
    ("git_https_rewrite", BOOL),
    ("group_report_output", BOOL),
    ("fallbacks", LIST),
    ("instructions", STR),
];

const DEFINITION: Shape = &[
    ("name", STR),
    ("workflow_id", STR),
    ("description", STR),
    ("routing_mode", STR),
    ("context_delivery", STR),
    ("execution_contract", STR),
    ("schema_version", INT),
    ("revision", INT),
    ("max_parallel", INT),
    ("max_transitions", INT),
    ("max_decision_attempts", INT),
    ("max_inspections", INT),
    ("max_on_demand_calls", INT),
    ("updated_at", NUMBER),
    ("orchestrator", DICT),
    ("nodes", LIST),
    ("connections", LIST),
    ("on_demand_nodes", LIST),
    ("resources", LIST),
];

const NODE: Shape = &[
    ("id", STR),
    ("type", STR),
    ("title", STR),
    ("purpose", STR),
    ("instructions", STR),
    ("prompt", STR),
    ("role", STR),
    ("freedom", STR),
    ("session_mode", STR),
    ("execution_mode", STR),
    ("branch_mode", STR),
    ("branch_selection", STR),
    ("selection_guidance", STR),
    ("parallel_group_id", STR),
    ("join_id", STR),
    ("workflow_name", STR),
    ("orchestrator_mode", STR),
    ("child_session_policy", STR),
    ("call_condition", STR),
    ("optional", BOOL),
    ("require_technical_plan", BOOL),
    ("require_tasks", BOOL),
    ("always_route", BOOL),
    ("max_attempts", INT),
    ("max_context_questions", INT),
    ("network", BOOL_OR_NULL),
    ("timeout_seconds", INT_OR_NULL),
    ("agent", DICT),
    ("position", DICT),
    ("workflow_ref", DICT),
    ("resources", LIST),
];

const CONNECTION: Shape = &[
    ("id", STR),
    ("source", STR),
    ("target", STR),
    ("condition", STR),
    ("default", BOOL),
    ("backward", BOOL),
    ("max_retries", INT),
];

const POSITION: Shape = &[("x", NUMBER), ("y", NUMBER)];

fn shape(value: &JsonObject, table: Shape, path: &str) -> Result<(), DocumentReason> {
    for (key, kinds) in table {
        if let Some(found) = value.get(key)
            && !kinds.contains(&kind_of(found))
        {
            let mut tail = String::from(path);
            tail.push('.');
            tail.push_str(key);
            return Err(DocumentReason::Malformed(tail));
        }
    }
    Ok(())
}

/// Whether every number is finite, every container reachable from `value` included.
fn all_finite(value: &Json) -> bool {
    let mut pending = Vec::from([value]);
    while let Some(item) = pending.pop() {
        match item {
            Json::Float(number) if !number.is_finite() => return false,
            Json::Array(items) => pending.extend(items.iter()),
            Json::Object(map) => pending.extend(map.iter().map(|(_, entry)| entry)),
            _ => {}
        }
    }
    true
}

fn object_of(value: &Json) -> Option<&JsonObject> {
    value.as_object()
}

fn candidate(value: &JsonObject) -> Result<(), DocumentReason> {
    shape(value, CANDIDATE, "agent")?;
    if let Some(Json::Array(paths)) = value.get("observe_repos") {
        let valid = paths.len() <= MAX_STORED
            && paths
                .iter()
                .all(|path| path.as_str().is_some_and(is_absolute));
        if !valid {
            return Err(DocumentReason::MalformedObserveRepos);
        }
    }
    if let Some(Json::Array(fallbacks)) = value.get("fallbacks") {
        for item in fallbacks {
            let entry = object_of(item).ok_or(DocumentReason::MalformedFallback)?;
            candidate(entry)?;
        }
    }
    Ok(())
}

fn node(value: &JsonObject) -> Result<(), DocumentReason> {
    shape(value, NODE, "node")?;
    if let Some(agent) = value.get("agent").and_then(object_of) {
        candidate(agent)?;
    }
    if let Some(reference) = value.get("workflow_ref").and_then(object_of) {
        let valid = reference
            .get("workflow_id")
            .and_then(Json::as_str)
            .is_some_and(|id| !is_blank(id));
        if !valid {
            return Err(DocumentReason::MalformedReference);
        }
    }
    if let Some(position) = value.get("position").and_then(object_of) {
        shape(position, POSITION, "position")?;
    }
    Ok(())
}

fn check_name(definition: &JsonObject, source_name: &str) -> Result<(), DocumentReason> {
    let own = definition
        .get("name")
        .and_then(Json::as_str)
        .filter(|name| is_workflow_name(name))
        .ok_or(DocumentReason::InvalidName)?;
    if !is_workflow_name(source_name) {
        return Err(DocumentReason::InvalidName);
    }
    if own != source_name {
        return Err(DocumentReason::NameMismatch);
    }
    Ok(())
}

/// `decode` without its envelope: finite values, the name match (only with `source_name`),
/// the type of every known field, fallbacks, observed paths, reference identities and
/// positions. A definition that passes can be written and read back.
pub fn qualify_stored(
    definition: &JsonObject,
    source_name: Option<&str>,
) -> Result<(), DocumentReason> {
    if !definition.iter().all(|(_, value)| all_finite(value)) {
        return Err(DocumentReason::NonFiniteValue);
    }
    if let Some(source) = source_name {
        check_name(definition, source)?;
    }
    shape(definition, DEFINITION, "definition")?;
    if let Some(orchestrator) = definition.get("orchestrator").and_then(object_of) {
        candidate(orchestrator)?;
    }
    for key in ["nodes", "on_demand_nodes"] {
        for item in definition
            .get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            node(object_of(item).ok_or(DocumentReason::MalformedNode)?)?;
        }
    }
    for item in definition
        .get("connections")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let edge = object_of(item).ok_or(DocumentReason::MalformedConnection)?;
        shape(edge, CONNECTION, "connection")?;
    }
    Ok(())
}
