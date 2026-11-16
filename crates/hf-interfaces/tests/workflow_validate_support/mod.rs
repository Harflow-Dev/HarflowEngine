//! Shared by the workflow validation tests: definition builders and validator helpers.
#![allow(dead_code, unused_imports)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../../hf-application/tests/workflow_support/mod.rs"]
pub mod workflow_support;

use hf_application::workflow::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_interfaces::workflow::json_text::parse;
use hf_interfaces::workflow::messages::failure_message;
pub use workflow_support::{MapResolver, TableCatalog};

/// Reads JSON text that the test wrote.
pub fn json(text: &str) -> Json {
    parse(text).expect("test JSON is valid")
}

/// The paths the resolver maps to themselves.
pub fn resolver() -> MapResolver {
    MapResolver::identity(&["/repo/a", "/repo/b", "/repo/c"])
}

/// Validates `input` and returns the normalized definition or the refusal message.
pub fn run(input: &Json) -> Result<JsonObject, String> {
    run_with(&resolver(), input)
}

/// As `run`, with the given path resolver.
pub fn run_with(resolver: &MapResolver, input: &Json) -> Result<JsonObject, String> {
    Validator::new(&TableCatalog, resolver)
        .validate(input)
        .map_err(|failure| failure_message(&failure))
}

/// The normalized candidate of `text` as `Validator::candidate` leaves it.
pub fn candidate(text: &str, grants: bool) -> Result<JsonObject, String> {
    let resolver = resolver();
    Validator::new(&TableCatalog, &resolver)
        .candidate(&json(text), grants)
        .map_err(|refusal| hf_interfaces::workflow::messages::message(&refusal))
}

/// The normalized definition of text that must validate.
pub fn accepted(text: &str) -> JsonObject {
    run(&json(text)).unwrap_or_else(|message| panic!("refused: {message}"))
}

/// The refusal message of text that must not validate.
pub fn refused(text: &str) -> String {
    match run(&json(text)) {
        Ok(_) => panic!("accepted a definition that must be refused"),
        Err(message) => message,
    }
}

fn joined(extra: &str) -> String {
    if extra.is_empty() {
        String::new()
    } else {
        format!(",{extra}")
    }
}

/// Start, one agent node `a` carrying `agent_fields`, End; top-level `top` fields added.
pub fn flow(top: &str, agent_fields: &str) -> String {
    format!(
        r#"{{"name":"flow"{top},"nodes":[{{"id":"s","type":"start"}},{{"id":"a","type":"agent"{agent}}},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"a"}},{{"id":"c2","source":"a","target":"e"}}]}}"#,
        top = joined(top),
        agent = joined(agent_fields),
    )
}

/// A flow whose agent node `a` has extra fields.
pub fn agent(agent_fields: &str) -> String {
    flow("", agent_fields)
}

/// A flow whose agent node `a` carries the candidate `settings` as its `agent`; `codex`
/// is the backend unless the settings name one.
pub fn with_agent(settings: &str) -> String {
    let body = if settings.contains("\"backend\"") {
        settings.to_string()
    } else if settings.is_empty() {
        r#""backend":"codex""#.to_string()
    } else {
        format!(r#""backend":"codex",{settings}"#)
    };
    agent(&format!(r#""agent":{{{body}}}"#))
}

/// Start, a middle node `a` given as complete JSON text, End; `top` fields added.
pub fn node_flow(top: &str, node: &str) -> String {
    format!(
        r#"{{"name":"flow"{top},"nodes":[{{"id":"s","type":"start"}},{node},{{"id":"e","type":"end"}}],"connections":[{{"id":"c1","source":"s","target":"a"}},{{"id":"c2","source":"a","target":"e"}}]}}"#,
        top = joined(top),
    )
}

/// As `node_flow` with explicit routing.
pub fn explicit_flow(node: &str) -> String {
    node_flow(r#""routing_mode":"explicit""#, node)
}

/// A flow with extra top-level fields.
pub fn top(top_fields: &str) -> String {
    flow(top_fields, "")
}

/// A value of the normalized definition by dotted path (numeric segments index arrays).
pub fn at<'a>(definition: &'a JsonObject, path: &str) -> Option<&'a Json> {
    let mut parts = path.split('.');
    let mut current = definition.get(parts.next()?)?;
    for part in parts {
        current = match current {
            Json::Object(map) => map.get(part)?,
            Json::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

pub fn keys(value: &Json) -> Vec<&str> {
    value
        .as_object()
        .map(|map| map.keys().collect())
        .unwrap_or_default()
}
