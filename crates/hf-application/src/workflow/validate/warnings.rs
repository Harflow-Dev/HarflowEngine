//! Advisories that never reject or rewrite a definition.
use hf_domain::workflow::json::{Json, JsonObject};

/// One warning per agent or workflow node without a `purpose`.
pub fn definition_warnings(definition: &JsonObject) -> Vec<String> {
    let nodes = definition
        .get("nodes")
        .and_then(Json::as_array)
        .into_iter()
        .flatten();
    nodes
        .filter_map(Json::as_object)
        .filter_map(|node| {
            let kind = node.get("type").and_then(Json::as_str)?;
            let has_purpose = node.get("purpose").is_some_and(|value| match value {
                Json::Null | Json::Bool(false) => false,
                Json::Str(text) => !text.is_empty(),
                Json::Array(items) => !items.is_empty(),
                Json::Object(map) => !map.is_empty(),
                Json::Int(number) => !number.is_zero(),
                Json::Float(number) => *number != 0.0,
                Json::Bool(true) => true,
            });
            if !matches!(kind, "agent" | "workflow") || has_purpose {
                return None;
            }
            let label = if kind == "agent" {
                "Agent "
            } else {
                "Workflow "
            };
            let id = node.get("id").and_then(Json::as_str).unwrap_or("");
            Some(format!(
                "{label}{id} has no purpose; context uses a bounded instructions excerpt"
            ))
        })
        .collect()
}
