//! Save-only defaults preserve raw field presence and historical node modes.
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::tree::{nodes, text};
use std::collections::HashMap;

pub(super) fn execution_modes(
    definition: &mut JsonObject,
    raw: &JsonObject,
    old: Option<&JsonObject>,
) {
    let raw_nodes: HashMap<_, _> = nodes(raw, "nodes")
        .map(|node| (text(node, "id"), node))
        .collect();
    let old_nodes: HashMap<_, _> = old
        .into_iter()
        .flat_map(|old| nodes(old, "nodes"))
        .map(|node| (text(node, "id"), node))
        .collect();
    if let Some(Json::Array(items)) = definition.get_mut("nodes") {
        for node in items.iter_mut().filter_map(Json::as_object_mut) {
            if text(node, "type") != "agent" {
                continue;
            }
            let id = text(node, "id");
            if raw_nodes
                .get(id)
                .is_some_and(|raw| raw.contains_key("execution_mode"))
            {
                continue;
            }
            let mode = match old_nodes.get(id) {
                Some(old) => old
                    .get("execution_mode")
                    .cloned()
                    .unwrap_or(Json::text("headless")),
                None if node.contains_key("resources") => Json::text("headless"),
                None => Json::text("prefer_subagent"),
            };
            node.insert("execution_mode", mode);
        }
    }
}
