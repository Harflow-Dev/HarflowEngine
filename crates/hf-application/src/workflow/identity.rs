//! Stable workflow identity.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::text::is_blank;

use super::ports::{DefinitionCodec, IdSource};

/// The id the engine owns; a definition without one reports a name-derived id.
pub fn workflow_identity(codec: &dyn DefinitionCodec, definition: &JsonObject) -> String {
    if let Some(Json::Str(id)) = definition.get("workflow_id")
        && !is_blank(id)
    {
        return id.clone();
    }
    let name = match definition.get("name") {
        Some(Json::Str(text)) => text.clone(),
        Some(Json::Int(value)) => value.text(),
        Some(Json::Bool(true)) => "True".to_string(),
        Some(Json::Bool(false)) => "False".to_string(),
        Some(Json::Null) => "None".to_string(),
        _ => String::new(),
    };
    let digest = codec.sha256_hex(name.as_bytes());
    let mut id = String::from("legacy-");
    id.push_str(digest.get(..32).unwrap_or(&digest));
    id
}

/// SHA-256 of a definition in canonical form.
pub fn definition_sha256(codec: &dyn DefinitionCodec, definition: &JsonObject) -> String {
    codec.definition_sha256(definition)
}

/// The authoritative id of a saved definition: preserved, legacy-derived, or minted.
pub fn persist_workflow_id(
    codec: &dyn DefinitionCodec,
    ids: &dyn IdSource,
    previous: Option<&JsonObject>,
) -> String {
    match previous {
        Some(old) => match old.get("workflow_id") {
            Some(Json::Str(id)) if !id.is_empty() => id.clone(),
            _ => workflow_identity(codec, old),
        },
        None => ids.workflow_id(),
    }
}
