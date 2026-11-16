//! Normalization of saved definitions before validation, and the read projection.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::rule_shape::DefinitionRule;

use super::field::{Check, refuse};
use crate::workflow::identity::workflow_identity;
use crate::workflow::ports::DefinitionCodec;

fn default_orchestrator() -> Json {
    Json::Object(JsonObject::from_entries(Vec::from([(
        "backend".to_string(),
        Json::text("codex"),
    )])))
}

/// Moves a Start prompt into the orchestrator's instructions and fills the defaults that
/// predate them. Works on a copy; a non-object orchestrator is left for validation.
pub fn migrate_orchestrator_instructions(definition: &JsonObject) -> Check<JsonObject> {
    let mut value = definition.clone();
    value.set_default("orchestrator", default_orchestrator());
    let Some(Json::Object(mut orchestrator)) = value.get("orchestrator").cloned() else {
        return Ok(value);
    };
    if let Some(Json::Array(nodes)) = value.get_mut("nodes") {
        for node in nodes.iter_mut().filter_map(Json::as_object_mut) {
            if node.get("type").and_then(Json::as_str) != Some("start") {
                continue;
            }
            if let Some(prompt) = node.get("prompt") {
                if prompt.as_str().is_none() {
                    return Err(refuse(DefinitionRule::StartPromptNotText));
                }
                if !orchestrator.contains_key("instructions") {
                    orchestrator.insert("instructions", prompt.clone());
                }
            }
            node.remove("prompt");
        }
    }
    orchestrator.set_default("instructions", Json::text(""));
    value.insert("orchestrator", Json::Object(orchestrator));
    value.set_default("on_demand_nodes", Json::Array(Vec::new()));
    value.set_default("max_on_demand_calls", Json::int(5));
    Ok(value)
}

/// What a read of a stored definition returns: migrated, with its identity and with
/// execution modes filled in on agent nodes that predate them.
pub fn project_definition(
    codec: &dyn DefinitionCodec,
    definition: &JsonObject,
) -> Check<JsonObject> {
    let mut value = migrate_orchestrator_instructions(definition)?;
    let identity = workflow_identity(codec, &value);
    value.set_default("workflow_id", Json::Str(identity));
    if let Some(Json::Array(nodes)) = value.get_mut("nodes") {
        for node in nodes.iter_mut().filter_map(Json::as_object_mut) {
            if node.get("type").and_then(Json::as_str) == Some("agent") {
                node.set_default("execution_mode", Json::text("headless"));
            }
        }
    }
    Ok(value)
}
