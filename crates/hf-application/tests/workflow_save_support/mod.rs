//! Shared construction for workflow save and catalog use-case tests.
#![allow(
    dead_code,
    unused_imports,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
#[path = "../workflow_support/mod.rs"]
pub mod workflow_support;

use hf_application::workflow::catalog::CatalogFailure;
use hf_application::workflow::ports::{AuthorityRefusal, Clock, IdSource, SaveAuthority};
use hf_application::workflow::save::{DefinitionSaver, SaveExpectations};
use hf_application::workflow::validate::Validator;
use hf_domain::workflow::json::{Json, JsonObject};
use hf_domain::workflow::tree::DependencyTree;
use workflow_support::{FixedClock, MapResolver, TableCatalog};

pub use workflow_support::{FakeCodec, MemStore, SeqIds, StoreCall, StoreOperation};

pub struct Allow;

impl SaveAuthority for Allow {
    fn admit(&self, _: &DependencyTree) -> Result<(), AuthorityRefusal> {
        Ok(())
    }
}

pub struct Deny(pub &'static str);

impl SaveAuthority for Deny {
    fn admit(&self, _: &DependencyTree) -> Result<(), AuthorityRefusal> {
        Err(AuthorityRefusal {
            message: self.0.to_string(),
        })
    }
}

pub fn save(
    store: &impl hf_application::workflow::ports::DefinitionStore,
    raw: &JsonObject,
    expected: &SaveExpectations,
    authority: &dyn SaveAuthority,
    ids: &dyn IdSource,
) -> Result<JsonObject, CatalogFailure> {
    save_at(store, raw, expected, authority, ids, &FixedClock(1234.5))
}

pub fn save_at(
    store: &impl hf_application::workflow::ports::DefinitionStore,
    raw: &JsonObject,
    expected: &SaveExpectations,
    authority: &dyn SaveAuthority,
    ids: &dyn IdSource,
    clock: &dyn Clock,
) -> Result<JsonObject, CatalogFailure> {
    let catalog = TableCatalog;
    let resolver = MapResolver::identity(&[]);
    let validator = Validator::new(&catalog, &resolver);
    let codec = FakeCodec;
    DefinitionSaver {
        validator: &validator,
        codec: &codec,
        ids,
        clock,
        authority,
    }
    .save(
        store,
        raw.get("name").and_then(Json::as_str).unwrap(),
        raw,
        expected,
    )
}

pub fn definition(name: &str) -> JsonObject {
    JsonObject::from_entries(vec![
        ("name".to_string(), Json::text(name)),
        (
            "nodes".to_string(),
            Json::Array(vec![
                node("start", "start", None),
                node("work", "agent", None),
                node("end", "end", None),
            ]),
        ),
        (
            "connections".to_string(),
            Json::Array(vec![edge("c1", "start", "work"), edge("c2", "work", "end")]),
        ),
    ])
}

pub fn reference_definition(name: &str, target_id: &str) -> JsonObject {
    let mut root = JsonObject::from_entries(vec![
        ("name".to_string(), Json::text(name)),
        ("routing_mode".to_string(), Json::text("explicit")),
    ]);
    let reference = Json::Object(JsonObject::from_entries(vec![(
        "workflow_id".to_string(),
        Json::text(target_id),
    )]));
    root.insert(
        "nodes",
        Json::Array(vec![
            node("start", "start", None),
            node("child", "workflow", Some(reference)),
            node("end", "end", None),
        ]),
    );
    root.insert(
        "connections",
        Json::Array(vec![
            edge("c1", "start", "child"),
            edge("c2", "child", "end"),
        ]),
    );
    root
}

pub fn node(id: &str, kind: &str, reference: Option<Json>) -> Json {
    let mut node = JsonObject::from_entries(vec![
        ("id".to_string(), Json::text(id)),
        ("type".to_string(), Json::text(kind)),
    ]);
    if let Some(reference) = reference {
        node.insert("workflow_ref", reference);
    }
    Json::Object(node)
}

pub fn edge(id: &str, source: &str, target: &str) -> Json {
    Json::Object(JsonObject::from_entries(vec![
        ("id".to_string(), Json::text(id)),
        ("source".to_string(), Json::text(source)),
        ("target".to_string(), Json::text(target)),
    ]))
}

pub fn refusal(name: &str) -> hf_domain::workflow::refusal::DocumentRefusal {
    use hf_domain::workflow::refusal::{DocumentCode, DocumentReason, DocumentRefusal};
    DocumentRefusal {
        name: name.to_string(),
        code: DocumentCode::Unavailable,
        reason: DocumentReason::Os("injected test failure".to_string()),
    }
}
