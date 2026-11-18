//! Save-only execution-mode defaults preserve explicit raw choices on existing nodes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod workflow_save_support;

use hf_application::workflow::save::SaveExpectations;
use hf_domain::workflow::json::{Json, JsonInt};
use workflow_save_support::{Allow, MemStore, SeqIds, definition, save};

fn resource_definition(name: &str, mode: &str) -> hf_domain::workflow::json::JsonObject {
    let mut raw = definition(name);
    let nodes = raw.get_mut("nodes").unwrap().as_array_mut().unwrap();
    let agent = nodes[1].as_object_mut().unwrap();
    agent.insert("resources", Json::texts(&["database".to_string()]));
    agent.insert("execution_mode", Json::text(mode));
    raw
}

#[test]
fn new_agent_prefers_subagent_but_explicit_raw_headless_replaces_it() {
    let store = MemStore::new();
    let ids = SeqIds::default();
    let first = save(
        &store,
        &definition("flow"),
        &SaveExpectations::default(),
        &Allow,
        &ids,
    )
    .unwrap();
    let first_agent = first.get("nodes").and_then(Json::as_array).unwrap()[1]
        .as_object()
        .unwrap();
    assert_eq!(
        first_agent.get("execution_mode"),
        Some(&Json::text("prefer_subagent"))
    );

    let saved = save(
        &store,
        &resource_definition("flow", "headless"),
        &SaveExpectations {
            workflow_id: first
                .get("workflow_id")
                .and_then(Json::as_str)
                .map(str::to_string),
            revision: Some(JsonInt::from_i64(1)),
        },
        &Allow,
        &ids,
    )
    .unwrap();
    let agent = saved.get("nodes").and_then(Json::as_array).unwrap()[1]
        .as_object()
        .unwrap();
    assert_eq!(agent.get("execution_mode"), Some(&Json::text("headless")));
}
