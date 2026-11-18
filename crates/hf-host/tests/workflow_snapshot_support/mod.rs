//! Shared builders for the A11 snapshot and resource-scope integration tests.
#![allow(dead_code, unused_imports)]
#[path = "../../../hf-application/tests/workflow_save_support/mod.rs"]
pub mod workflow_save_support;
pub use workflow_save_support::definition;

use hf_application::workflow::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::tree::{AccessSummary, DependencyTree, TreeEdge, TreeEntry};
use hf_domain::workflow::vocab::Freedom;
use workflow_save_support::workflow_support::{MapResolver, TableCatalog};

static CATALOG: TableCatalog = TableCatalog;
static RESOLVER: std::sync::OnceLock<MapResolver> = std::sync::OnceLock::new();

pub fn validator() -> Validator<'static> {
    Validator::new(
        &CATALOG,
        RESOLVER.get_or_init(|| MapResolver::identity(&[])),
    )
}

pub fn object(entries: &[(&str, Json)]) -> JsonObject {
    JsonObject::from_entries(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    )
}

pub fn orchestration(definition: &mut JsonObject, settings: &[(&str, Json)]) {
    let mut orchestrator = object(&[
        ("backend", Json::text("codex")),
        ("instructions", Json::text("run")),
    ]);
    for (key, value) in settings {
        orchestrator.insert(key, value.clone());
    }
    definition.insert("orchestrator", Json::Object(orchestrator));
}

pub fn entry(id: &str, name: &str, definition: JsonObject, hash: &str) -> (String, TreeEntry) {
    (
        id.to_string(),
        TreeEntry {
            name: name.to_string(),
            revision: Json::int(1),
            definition_sha256: hash.to_string(),
            definition,
        },
    )
}

pub fn tree(
    root_id: &str,
    entries: Vec<(String, TreeEntry)>,
    edges: Vec<TreeEdge>,
) -> DependencyTree {
    let access = entries
        .iter()
        .map(|(id, _)| {
            (
                id.clone(),
                AccessSummary {
                    max_freedom: Freedom::ReadOnly,
                    network: false,
                    backends: vec![],
                    depth: 1,
                },
            )
        })
        .collect();
    DependencyTree {
        root_workflow_id: root_id.to_string(),
        workflows: entries,
        edges,
        access,
    }
}

pub fn resource_definition(name: &str, declared: Option<&[&str]>, nodes: Vec<Json>) -> JsonObject {
    let mut result = object(&[("name", Json::text(name)), ("nodes", Json::Array(nodes))]);
    if let Some(resources) = declared {
        result.insert(
            "resources",
            Json::texts(
                &resources
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect::<Vec<_>>(),
            ),
        );
    }
    result
}

pub fn resource_node(id: &str, names: &[&str]) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        (
            "resources",
            Json::texts(
                &names
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect::<Vec<_>>(),
            ),
        ),
    ]))
}

pub fn workflow_node(id: &str, target: &str) -> Json {
    Json::Object(object(&[
        ("id", Json::text(id)),
        ("type", Json::text("workflow")),
        (
            "workflow_ref",
            Json::Object(object(&[("workflow_id", Json::text(target))])),
        ),
    ]))
}
